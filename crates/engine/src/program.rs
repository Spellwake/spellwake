//! Authored characteristics and executable capabilities under one definition owner.
//! Constructors assemble authored input. EffectWindow validates the complete
//! record before accepting it into the sole program map.
use crate::{
    activated::ActivatedAbility,
    characteristics::{
        CharacteristicDefinitionError, Color, KeywordAbility, PowerToughness,
        PowerToughnessCounters, PrintedPowerToughness, ResolvedDuration, ResolvedPartsSpec,
        StaticContinuousAbility,
    },
    land::LandType,
    life::LifeAdjustment,
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
            | Self::Multikicker(cost)
            | Self::Replicate(cost)
            | Self::Buyback(cost) => cost,
        }
    }
}
/// A permission on the physical creature card in its owner's graveyard.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GraveyardSelfCast {
    Unconditional,
    WhileControllingSubtype(CreatureTypeId),
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
            | Self::Escape { mana: price, .. } => price,
        }
    }
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
    /// Charge counters with which this permanent enters.
    FixedCharge(RulesInteger),
    /// Additional entry loyalty placed by a printed ability, on any permanent type.
    FixedLoyalty(RulesInteger),
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
    CardTypeMask, ColorMask, CombatFilter, ControllerFilter, EffectSubject, ManaValueFilter,
    ObjectClause, ObjectColorFilter, ObjectFilter, ObjectFilterFlags, PowerFilter, SourceRelation,
    SubtypeFilter, SupertypeMask, ToughnessFilter,
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
    PlayerStatExpr, StatAggregate, ValueExpr, ValueObjectRef, ValueZone,
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
enum BodyContext<'a> {
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
    const ACTIVATED: Self = Self::Activated {
        named_counter_cost: false,
    };
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
    /// This consumes the body's required [`BodyTarget::SingleSpell`] target.
    /// A departed target spell supplies its controller through last known information.
    TargetSpellController,
    /// The controller captured from the targeted permanent immediately before its exile.
    ExiledTargetController,
    /// The controller observed immediately before the preceding destruction instruction.
    DestructionTargetController,
    /// The body's exact bound player, consuming [`BodyTarget::SinglePlayer`].
    /// A creature-or-player target does not guarantee this player capability.
    TargetPlayer,
}
impl ResolvingPlayer {
    fn validate(
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
                        TriggerGrouping::EachOccurrence | TriggerGrouping::OneOrMoreByPlayer => {
                            true
                        }
                        TriggerGrouping::OneOrMoreTogether
                        | TriggerGrouping::OneOrMoreOnceEachTurn => false,
                    };
                    if singular && condition.captures_related_player() {
                        Ok(None)
                    } else {
                        Err(DefinitionError::UnguaranteedRelatedPlayer)
                    }
                }
            },
            Self::TargetSpellController => match target {
                BodyTarget::SingleSpell(_) => Ok(Some(OperandFamily::Spell)),
                BodyTarget::None
                | BodyTarget::ControlledAbility { .. }
                | BodyTarget::StackEntryTargetingControlledPermanent
                | BodyTarget::SingleStackEntry
                | BodyTarget::SinglePermanent
                | BodyTarget::SingleCreature
                | BodyTarget::SingleOpponent
                | BodyTarget::SinglePlayer
                | BodyTarget::SingleOwnGraveyardCard
                | BodyTarget::FilteredOwnGraveyardCard(_)
                | BodyTarget::AnyGraveyardCreatureOrPlaneswalker
                | BodyTarget::RecentlyDiscardedOrFromLibrary
                | BodyTarget::AnyGraveyardCard
                | BodyTarget::AnyGraveyardCreature
                | BodyTarget::OwnedExiledFlashbackCard
                | BodyTarget::FilteredPermanent(_)
                | BodyTarget::SingleCreatureOrPlayer
                | BodyTarget::SinglePlayerOrPlaneswalker
                | BodyTarget::OpponentOrPlaneswalkerThenCreature
                | BodyTarget::SinglePlaneswalker
                | BodyTarget::SingleAnyTarget => Err(DefinitionError::ResolvingPlayerTarget),
            },
            Self::TargetPlayer => match target {
                BodyTarget::SingleOpponent | BodyTarget::SinglePlayer => {
                    Ok(Some(OperandFamily::Player))
                }
                BodyTarget::None
                | BodyTarget::ControlledAbility { .. }
                | BodyTarget::StackEntryTargetingControlledPermanent
                | BodyTarget::SingleStackEntry
                | BodyTarget::SinglePermanent
                | BodyTarget::SingleCreature
                | BodyTarget::SingleOwnGraveyardCard
                | BodyTarget::FilteredOwnGraveyardCard(_)
                | BodyTarget::AnyGraveyardCreatureOrPlaneswalker
                | BodyTarget::RecentlyDiscardedOrFromLibrary
                | BodyTarget::AnyGraveyardCard
                | BodyTarget::AnyGraveyardCreature
                | BodyTarget::OwnedExiledFlashbackCard
                | BodyTarget::FilteredPermanent(_)
                | BodyTarget::SingleCreatureOrPlayer
                | BodyTarget::SinglePlayerOrPlaneswalker
                | BodyTarget::OpponentOrPlaneswalkerThenCreature
                | BodyTarget::SinglePlaneswalker
                | BodyTarget::SingleAnyTarget
                | BodyTarget::SingleSpell(_) => Err(DefinitionError::ResolvingPlayerTarget),
            },
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
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SacrificeSelection {
    Subtype(CreatureTypeId),
    NontokenCreature,
}

impl SacrificeSelection {
    pub(crate) fn matches(self, permanent: &crate::characteristics::ObjectCharacteristics) -> bool {
        match self {
            Self::Subtype(kind) => permanent.current_creature_subtypes.contains(&kind),
            Self::NontokenCreature => permanent.is_creature() && !permanent.is_token,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PermanentTypeFilter {
    /// When populated, at least one of these evaluated types must be present.
    pub any_types: BTreeSet<CardType>,
    /// Every listed type must occur in the permanent's evaluated current types.
    pub required_types: BTreeSet<CardType>,
    /// None of these types may occur in its evaluated current types.
    pub forbidden_types: BTreeSet<CardType>,
}
impl PermanentTypeFilter {
    /// Match evaluated types only; callers separately require current physical
    /// Battlefield membership and the affected player's control.
    pub(crate) fn matches_types(&self, current: &BTreeSet<CardType>) -> bool {
        (self.any_types.is_empty() || !self.any_types.is_disjoint(current))
            && self.required_types.is_subset(current)
            && self.forbidden_types.is_disjoint(current)
    }
    fn validate(&self) -> Result<(), DefinitionError> {
        if self.required_types.is_disjoint(&self.forbidden_types)
            && (self.any_types.is_empty() || !self.any_types.is_subset(&self.forbidden_types))
        {
            Ok(())
        } else {
            Err(DefinitionError::InvalidInstruction)
        }
    }
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
}

/// Cardinality policy for a library search.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SearchCount {
    /// Find up to `n` matching cards (fail-to-find permitted).
    UpTo(u8),
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
            && matches!(self.count, SearchCount::UpTo(n) if n > 1)
    }

    /// Whether this search uses the multi-card exile batch reply.
    #[must_use]
    pub fn is_exile_batch(&self) -> bool {
        matches!(self.destination, SearchDestination::Exile { .. })
            && matches!(self.count, SearchCount::UpTo(n) if n > 1)
    }

    /// Fixed maximum selection count for bounded hand or exile batch searches.
    #[must_use]
    pub fn fixed_batch_limit(&self) -> Option<usize> {
        match self.destination {
            SearchDestination::Hand | SearchDestination::Exile { .. } => match self.count {
                SearchCount::UpTo(n) if n > 1 => Some(usize::from(n)),
                SearchCount::Exact(n) if n.get() > 1 => Some(usize::from(n.get())),
                _ => None,
            },
            _ => None,
        }
    }

    /// Whether this search may be wrapped directly in an `OptionalEffect`.
    #[must_use]
    pub fn supports_optional_wrapper(&self) -> bool {
        !matches!(
            self.source,
            SearchSource::PlayerLibrary(_) | SearchSource::TargetOpponentLibrary
        ) && !self.is_required()
            && !self.is_transfigure()
            && !self.is_haste_and_return()
            && matches!(
                self.destination,
                SearchDestination::Hand
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

/// Qualification filter for an optional top-library reveal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LookTopFilter {
    NoncreatureNonland,
    Enchantment,
    Land,
    AllOfChosenType(CreatureTypeSlot),
    SourceChosenCreatureType,
    SourceChosenCreatureTypeKeepRest,
}

/// Recipients of one simultaneous nontargeted damage event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RevealedHandSelection {
    AllLands,
    OneCreature,
}

/// One simultaneous damage event with a group of recipients.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DamageGroup {
    Controller,
    ControllerAndTargetCreature,
    DestroyedTargetController,
    RelatedPlayer,
    RelatedPlayerOrTheirPlaneswalker,
    VolunteeringOpponent,
    CapturedAttackedPlayerOrPlaneswalker,
    /// Creatures whose current types do not include the vocabulary-bound subtype.
    CreaturesExcept(CreatureTypeId),
    CreaturesOfSubtype(CreatureTypeId),
    CreaturesWithoutFlying,
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
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PermanentTargetAction {
    /// Destroy the currently legal, destructible members together.
    Destroy,
    ReturnToHand,
    Tap,
    TapAndSkipNextUntap,
    Untap,
    /// Change the selected permanent's tap state if the optional effect is accepted.
    ToggleTap,
    PutPlusOneCounter,
    PutPlusOneCounterOnOthers,
    Exile,
    PutCountersByCurrentType,
    ModifyWithPartner {
        parts: ResolvedPartsSpec,
        duration: ResolvedDuration,
    },
    /// A temporary continuous effect on the one still-legal selected permanent.
    Modify {
        parts: ResolvedPartsSpec,
        duration: ResolvedDuration,
    },
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
}
/// A resolving attack trigger alters declaration choices for this combat.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockDeclarationRule {
    EachOpponentOneBlocker,
    ControllerChooses,
}
/// Recipients of a resolving instruction that grants protection from one chosen color.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProtectionRecipients {
    TargetPermanent,
    ControllerAndPermanents,
}

/// A unit of cost applied to each creature attacking the affected defender.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AttackCostSymbol {
    OneGeneric,
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
    ControlledCreatures,
    SourceAndControlledSubtype(CreatureTypeId),
    AttackersIfSourceAttacksHighestLife,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EntryCopyRule {
    GraveyardCreature,
    ControlledCreatureOrPlaneswalker,
    BattlefieldCreatureWithAbility(Box<crate::activated::ActivatedAbility>),
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
    pub const fn from_trigger_condition(cond: TriggerCondition) -> Option<Self> {
        match cond {
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

/// Mana produced during spell or ability resolution by [`Instruction::AddMana`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EffectManaYield {
    /// Add a fixed mana bundle.
    Fixed(BTreeMap<crate::mana::ManaType, RulesInteger>),
    /// Add mana equal to the triggering combat damage; only those units survive step changes this turn.
    CombatDamageUntilCleanup(crate::mana::ManaType),
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
    /// Choose a color from `colors` and add one mana restricted to `restriction`.
    RestrictedChoice {
        colors: BTreeSet<crate::mana::ManaType>,
        restriction: crate::mana::ManaRestriction,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Instruction {
    ControlManaSourceThroughNextTurn,
    /// Allow the resolving controller to cast spells as though they had flash through cleanup.
    GrantControllerFlashUntilCleanup,
    AddExperienceCounter,
    SuppressGroupKeywords {
        group: Box<EffectSubject>,
        keywords: BTreeSet<crate::characteristics::KeywordAbility>,
    },
    GoadTarget,
    /// This creature must attack this turn if able, with no restriction on its defender.
    RequireTargetAttack,
    /// Remove an attacking or blocking creature from combat, preserving its zone and status.
    RemoveTargetFromCombat,
    /// Reveal your top card, remove the captured attacker if it's a Forest, then bottom it.
    RevealTopAndRemoveAttackerIfForest,
    RegenerateTarget,
    RegenerateSource,
    /// Create a regeneration shield for each controlled creature of this subtype.
    RegenerateControlledSubtype(CreatureTypeId),
    /// Untap the permanent that activated the resolving ability.
    UntapAbilitySource,
    TapAbilitySource,
    /// Tap a target and suspend its normal untap while this trigger's controller retains its source.
    TapTargetWhileControllingSource,
    /// Transform the ability's original permanent if it has not transformed since announcement.
    TransformSource,
    /// Transform the source and attach its new face to the captured target.
    TransformSourceAndAttachToTarget,
    /// Transform eligible battlefield permanents of this subtype simultaneously.
    TransformSubtype(CreatureTypeId),
    /// The original ability source and the targeted creature fight.
    FightSourceWithTarget,
    /// Remove all counters of a named kind from the original ability source.
    ClearSourceCounters(crate::effects::CounterKind),
    /// Place a +1/+1 counter on every other creature controlled by the ability controller.
    PutCountersOnOtherCreatures,
    PutCountersOnControlledCreatures,
    PutCountersOnControlledSubtype(CreatureTypeId),
    PutCountersOnControlledCreaturesAndGrant(BTreeSet<crate::characteristics::KeywordAbility>),
    /// Put a +1/+1 counter on the permanent captured by the current trigger.
    PutCounterOnRelatedPermanent,
    /// Encore creates a source copy for each opponent and assigns its attack and sacrifice requirements.
    Encore,
    Myriad,
    CopyCreatureAsToken,
    /// Copy the exact entrant, once per controller of this ability per turn.
    GiveEquippedCreatureToUpkeepPlayer,
    CopyEntrantOnceEachTurn,
    CopyEntrantAsArtifact,
    /// Exile a graveyard card and create a modified copy from its copiable values.
    ExileForBlackToken {
        subtype: CreatureTypeId,
        size: u8,
        add_types: bool,
    },
    /// Exile a graveyard creature to copy its values with one additional subtype.
    ExileForSubtypeToken(CreatureTypeId),
    /// Copy the creature card chosen and exiled to pay the enclosing activation cost.
    CopyCreatureExiledForAbility,
    /// Deal damage from the permanent that granted this activated ability.
    DamageFromGrantingPermanent {
        amount: RulesInteger,
    },
    /// Calculate a subtype count once for the life-loss and scry instructions.
    LoseLifeAndScryForSubtype(CreatureTypeId),
    ReturnDelayedGraveyardToHand(crate::object_identity::ObjectId),
    ReplaceSourceCreatedTokens,
    GrantNewTokensHaste,
    /// Delay exile of the actual tokens produced by the preceding instruction.
    ScheduleCreatedTokenExile(Step),
    /// Sacrifice the immediately preceding token cohort at the next end step.
    ScheduleCreatedTokenSacrifice,
    ExileDelayedObjects(Vec<crate::object_identity::ObjectId>),
    /// Exile a live group derived from the target simultaneously.
    ExileRelatedPermanents(RelatedExileGroup),
    /// Exile a creature under a source link, even if that source has already left play.
    ExileTargetWithLink(LinkedExileGroup),
    /// Return cards attributed to this exact source and its printed link.
    ReturnCardsExiledWithLink(LinkedExileGroup),
    /// Destroy every permanent of this enchantment subtype attached to the resolving controller.
    DestroyAttachedEnchantmentSubtype(EnchantmentSubtype),
    /// Optionally attach a controlled Equipment to one token created by the preceding instruction.
    EquipNewToken,
    /// Put up to one Equipment card from hand or graveyard onto the battlefield.
    PutEquipmentFromHandOrGraveyard,
    /// Life paid by the resolving controller as the first part of a chosen optional sequence.
    PayControllerLife(u16),
    /// Put +1/+1 counters on controlled creatures and loyalty counters on other controlled planeswalkers.
    PutCreatureAndOtherPlaneswalkerCounters,
    /// Add one charge counter to the source if its exact incarnation remains on the battlefield.
    PutChargeCounterOnSource,
    /// Place a blessing counter on this ability’s current source.
    PutBlessingCounterOnSource,
    /// Put one named counter on the current source of this ability, if still present.
    PutNamedCounterOnSource {
        kind: crate::effects::CounterKind,
        amount: u16,
    },
    /// Put one charge counter on this source for each point of the announced X.
    PutChosenXChargeCounters,
    /// Destroy each nonland permanent matching the sacrificed source’s charge counter count.
    DestroyNonlandsAtSourceChargeCount,
    /// Destroy the nonlands of mana value X controlled by players dealt combat damage by this source this turn.
    DestroyDamagedPlayersNonlandsAtX,
    PutQuestCounterOnSource,
    PutStorageCounterOnSource,
    /// The participant captured by a damage trigger loses half their current life, rounded up.
    RelatedPlayerLosesLife(u8),
    /// Current or last known controller of this activated ability’s source loses life.
    AbilitySourceControllerLosesLife(u8),
    RelatedPlayerLosesHalfLife,
    LinkedPlayerLosesDamageAmount,
    /// Return the permanent that produced this ability to its physical owner’s hand.
    /// Exile the triggering permanent and gain life if the exile cost was paid.
    ExileSourceAndGainLife(u8),
    /// Exile the Siege whose last defense counter was removed.
    ExileDefeatedSiege,
    /// Offer the transformed face of the physical card exiled by this resolution.
    CastDefeatedSiege,
    ReturnSourceToHand,
    GoadOtherPlayersCreatures,
    /// Exile the controller's top card; they may cast it normally, otherwise deal this much damage to each opponent.
    ImpulseCastOrDamage(RulesInteger),
    /// Engine-owned private exile of the controller's top card.
    ExileTopFaceDown,
    /// Exile the top card and allow its controller to play it this turn or until the next successful exile by this source.
    ExileTopForPlay {
        until_next_from_source: bool,
    },
    /// Until cleanup the controller may cast noncreature spells from the resulting exile cohort for free.
    ExileTopFreeNoncreatureSpells(u8),
    ExileTopForEachSpellType(u8),
    PutLandFromExiledCohort,
    /// Choose one matching public graveyard card and put it onto the battlefield.
    PutGraveyardCreatureAtMostX,
    /// Return a chosen permanent card of odd mana value when the actual cast paid the threshold.
    ReturnOddGraveyardPermanentIfPaid {
        mana: crate::mana::ManaType,
        minimum: u8,
    },
    RevealCreatureForEachTargetController,
    /// Return the actual cards exiled by the preceding target-group instruction.
    ReturnExiledTargetCohort {
        next_end_step: bool,
        owner_controls: bool,
    },
    /// Return the physical cards the previous destruction put into a graveyard.
    ReturnCardsDestroyedThisWay,
    /// Runtime-bound cards returning under their owners with type-specific counters.
    ReturnDelayedExileWithCounters(Vec<crate::object_identity::ObjectId>),
    /// Shuffle one announced permanent for each participating player.
    ShuffleOnePermanentPerPlayer,
    /// Starting with the controller, vote for a matching graveyard card and return all that tie for the most votes.
    VoteForGraveyardCards {
        types: BTreeSet<CardType>,
    },
    /// Exile up to this many creatures actually milled into graveyards by the preceding instruction.
    ExileMilledCreatures {
        maximum: u16,
    },
    /// Each former controller exiles through a nonland, bottoms the rest and may cast it.
    ExileNonlandForFormerControllers,
    /// Sacrifice one of the exact controlled creatures in this combat damage group.
    SacrificeCombatContributor,
    /// Reveal until a creature shares a type with the sacrificed permanent’s last known appearance.
    RevealCreatureOfSacrificedType,
    /// Play the exiled top card of the captured player’s library with any color of mana.
    ExileRelatedTopForPlay,
    ExileRelatedTopForCastThisTurn,
    /// Register a delayed return for the token and exile from this resolution.
    RegisterTokenLinkedExile,
    /// Engine-owned delayed return of the exact linked exiled incarnation.
    ReturnTokenLinkedExile,
    /// Add mana during spell or ability resolution.
    AddMana(EffectManaYield),
    /// Guard an intervening-if or resolution-time condition.
    RequireCondition(InterveningCondition),
    /// Archenemy ongoing scheme abandon action (moves source scheme to the bottom of its owner's scheme deck).
    AbandonSourceScheme,
    /// Opt-in paper Un-card physical or social tabletop honor-system action.
    TabletopHonorAction {
        prompt: String,
    },
    /// Remove one loyalty counter from any number of controlled permanents, then exile that many top cards for play this turn.
    RemoveLoyaltyExileTopForPlay,
    /// Create a separately targeted damage trigger if this ability discarded a nonland card.
    CreateReflexiveDiscardDamage,
    /// Copy the trigger source with the nonlegendary and starting-loyalty exceptions.
    CreateNonlegendaryLoyaltyCopy,
    /// Reveal the actual card drawn, then remove loyalty equal to its mana value.
    DrawRevealRemoveLoyalty,
    WeakenOpposingAttackersUntilNextTurn,
    PhaseOtherPlaneswalkersNextEndStep,
    PhaseOut(PhasingSubject),
    /// Choose one named creature to exile with the exact source, then meld the two actual printed cards.
    ChooseMeldPartner(spellwake_contracts::names::EnglishCardName),
    ExileMeldPair,
    MeldExiledPair(Box<Program>),
    PairWithCreature {
        entrant_only: bool,
    },
    PhaseOutObjects(Vec<crate::object_identity::ObjectId>),
    ReturnOneNonlandPerOpponent,
    WeakenDelayedAttacker(crate::object_identity::ObjectId),
    /// Deal the resolved amount to each member of the group simultaneously.
    DamageOpponentsAmount(ValueExpr),
    DamageRelatedPlayerAmount(ValueExpr),
    /// Deal the captured combat damage to each creature currently controlled by the damaged player.
    DamageDamagedPlayersCreatures,
    /// Create a persistent command-zone emblem controlled by the resolving player.
    CreateEmblem(crate::emblem::EmblemDefinition),
    /// Each opponent receives an independent emblem.
    CreateOpponentEmblems(crate::emblem::EmblemDefinition),
    /// Destroy all matching destructible permanents in one zone-change event.
    DestroyGroup(Box<ObjectFilter>),
    DestroyGroupWithoutRegeneration(Box<ObjectFilter>),
    /// Return the matching cohort to owners’ hands or exile it simultaneously.
    MoveGroup {
        group: Box<ObjectFilter>,
        destination: PublicZoneDestination,
    },
    /// Move the announced permanent or graveyard card to its owner's library.
    PutTargetInLibrary(LibraryPlacement),
    /// Choose one card from the resolving controller's hand and put it on their library bottom.
    PutOwnHandCardOnLibraryBottom,
    /// Move the whole battlefield cohort to its owners' library bottoms.
    PutGroupOnLibraryBottom(Box<ObjectFilter>),
    /// Exile the announced card from any graveyard.
    ExileGraveyardTarget,
    ExileCapturedGraveyardCard,
    /// Allow the controller to cast this exact graveyard card this turn and watch its cast.
    GrantGraveyardCastAndWatchAbilities,
    /// Runtime instruction produced by the matching delayed trigger.
    GainCastCardAbilities {
        recipient: crate::object_identity::ObjectId,
        donor: Box<crate::characteristics::ObjectCharacteristics>,
    },
    ExileCardsFromGraveyard {
        player: ResolvingPlayer,
        count: std::num::NonZeroU8,
    },
    /// Create a continuous damage-prevention effect lasting through this turn.
    PreventDamageThisTurn(crate::damage::TurnPreventionFilter),
    /// The resolving controller takes a turn immediately after the current turn.
    TakeExtraTurn,
    /// Untap the current attackers and schedule a new combat phase after this one.
    AdditionalCombatAfterUntappingAttackers,
    AdditionalCombat {
        untap: CombatUntapGroup,
    },
    /// Its controller may choose new targets for the targeted spell or ability (CR 115.7d).
    ChooseNewTargets,
    /// Transfer control of the targeted spell, keeping its owner, caster and stack incarnation.
    GainControlOfSpell,
    ChangeOnlyTarget,
    LoseLifeEqualToTargetSpellManaValue,
    /// Capture all matching identities in one continuous effect through end of turn.
    SetControlledCreatureSizeToHand,
    /// Creatures the spell’s opponents control now must block this turn if able.
    RequireOpponentsCreaturesBlockThisTurn,
    /// Each creature must block the targeted creature this turn if able.
    RequireBlocksAgainstTargetThisTurn,
    ModifyGroupByAmount {
        group: Box<EffectSubject>,
        scale: PowerToughness,
        amount: ValueExpr,
        keywords: BTreeSet<KeywordAbility>,
    },
    ModifyGroupUntilEndOfTurn {
        group: Box<EffectSubject>,
        power_toughness: Option<crate::characteristics::PowerToughness>,
        keywords: BTreeSet<crate::characteristics::KeywordAbility>,
    },
    /// Give this activated ability’s same-incarnation source a boost until end of turn.
    BoostSource {
        power_toughness: Option<crate::characteristics::PowerToughness>,
        keywords: BTreeSet<KeywordAbility>,
    },
    CopyOnCast(CastCopyKeyword),
    WatchNextLoyaltyCopy,
    WatchNextSpellOrLoyaltyTwice,
    CopyResolvingSpell,
    CopyForOtherCreatures,
    ChoosePlaneswalkerTypeForCopies,
    /// Only a delayed trigger with captured ability bindings can execute this operation.
    CopyCapturedAbility,
    /// The resolving controller may put plus-one counters on this trigger's exact source.
    /// Each branch continues at its specified forward instruction (CR 118.12).
    OfferSourceCounters {
        amount: RulesInteger,
        placed_destination: usize,
        declined_destination: usize,
    },
    /// Search the target player's library for a card and exile it.
    ExileFromTargetLibrary,
    ExileLibraryTop {
        player: ResolvingPlayer,
        amount: u16,
    },
    /// Manifest cards from the player’s library, one at a time.
    ManifestTop {
        player: ResolvingPlayer,
        amount: ValueExpr,
    },
    /// Put any number of face-up exiled lands owned by the targeted opponent onto the battlefield.
    PutTargetOwnedExiledLandsOntoBattlefield,
    /// Ordered binding for the following operations, including optional targets.
    BindOrderedPermanents {
        minimum: u16,
        maximum: u16,
    },
    /// Exile the legal permanent announced at this zero-based position.
    ExileOrderedPermanent(u16),
    /// One mandatory target clause in a fixed sequence; the enclosed body uses that clause's target.
    FixedTargetClause(Box<ExecutableBody>),
    /// Exile the resolving spell itself after the preceding instructions complete.
    ExileResolvingSpell,
    /// Destroy all surviving independently bound permanent targets as one event.
    DestroyFixedTargets,
    /// Destroy the target and Equipment currently attached to it in one event.
    DestroyTargetAndAttachedEquipment,
    /// Return the still-legal members of distinct printed clauses in one zone event.
    ReturnFixedTargets,
    /// The two separately announced, distinct creatures fight if both remain legal.
    FightFixedTargets,
    /// Exile the first target until the second targeted permanent leaves.
    ExileUntilOtherTargetLeaves,
    /// The first target copies the second until end of turn and retains artifact as an additional type.
    CopyOtherTargetAsArtifact,
    /// Select a whole graveyard cohort and move it to the battlefield in one event.
    ReturnGraveyardCohort(GraveyardCohortReturn),
    /// Return the creature from a captured death under the ability controller's control.
    ReturnCapturedDyingCreature,
    ReturnCapturedDyingCreatureWithTypeAndColor {
        subtype: CreatureTypeId,
        color: crate::characteristics::Color,
    },
    /// Return the exact target, then place counters if its new incarnation has the required subtype.
    ReturnGraveyardCreatureWithSubtypeCounters {
        subtype: CreatureTypeId,
        amount: u16,
    },
    /// Return X announced own-graveyard creatures, then sacrifice the returned objects next end step.
    ReturnChosenXGraveyardCreatures,
    ReturnGraveyardTargetsTapped(u8),
    /// Simultaneous noncombat damage to one required and up to two other targets.
    /// Split three damage among one to three announced targets, giving the first any remainder.
    DivideThreeDamageAmongTargets,
    /// Place the +1/+1 counters divided among creature targets at announcement.
    DistributePlusOneCounters {
        total: u16,
    },
    DamageOrderedTargets {
        first: RulesInteger,
        others: RulesInteger,
    },
    /// Inspect a bounded top segment and optionally reveal one qualifying card.
    LookTopReveal {
        count: u8,
        filter: LookTopFilter,
    },
    /// Look at the top card and optionally put it onto the battlefield tapped if it is a land.
    LookTopLandBattlefieldTapped,
    /// Reveal the top card; optionally put a permanent with mana value at most three onto the battlefield, otherwise into hand.
    RevealTopPermanentOrHand,
    /// Look at seven, optionally put up to two planeswalkers onto the battlefield, then randomly bottom the rest.
    LookTopPlaneswalkers,
    /// Choose an opponent on resolution and draw their excess hand size.
    DrawToChosenOpponentHandSize,
    /// Reveal the top five, have an opponent divide them, then take one pile and graveyard the rest.
    RevealDividePiles,
    DivideTargetPermanentsAndSacrificePile,
    /// Divide three revealed cards, put one pile into hand, and order the other on the bottom.
    RevealHandAndBottomPiles,
    /// Privately look at this many cards, take one and put the rest into the graveyard.
    LookTopOneRestGraveyard {
        count: u8,
    },
    MayRevealLibraryTop,
    /// Reveal this many cards, move the eligible hand cohort, and put the rest in the graveyard.
    RevealTopHandRestGraveyard {
        count: u8,
        hand: RevealedHandSelection,
    },
    /// Privately choose one of the top cards for hand, then order the rest on the bottom.
    LookTopOneRestBottom {
        count: ValueExpr,
    },
    /// Privately look at the top cards, optionally put a matching creature onto the battlefield, and order the rest on the bottom.
    LookTopCreatureSharingAttachedSubtype {
        count: u8,
    },
    /// Reveal the top cards, take every creature and land, then order the rest on the bottom.
    RevealTopCreaturesAndLandsRestBottom {
        count: u8,
    },
    SearchEachLibraryForFreeCast,
    ChooseOwnCommanderToCast,
    /// Offer a free cast from hand that shares a card type with the last spell targeted for countering.
    ChooseMatchingHandSpellToCast,
    WatchTargetCombatDamageDraw,
    WatchSmallCreatureCombatDraw(u8),
    ChooseFoundSpell,
    /// Offer a free cast from cards exiled with this source having the announced mana value.
    CastLinkedSpellMatchingX,
    CastFoundSpell(crate::object_identity::ObjectId),
    /// An opponent chooses a revealed creature to enter while the rest go to the graveyard.
    RevealOpponentChoosesCreature,
    /// Shuffle the exact battlefield ability source into the owner's library.
    ShuffleAbilitySourceIntoLibrary,
    /// After paying the source shuffle, reveal through this name and put the rest in the graveyard.
    RevealToNameAfterSourceShuffle(spellwake_contracts::names::EnglishCardName),
    /// Choose land or nonland, reveal to the first match, take it, and randomly bottom the rest.
    RevealUntilLandKind,
    /// Reveal the top card, optionally cast it for free if its mana value is odd, or draw.
    RevealOddCastOrDraw,
    /// Reveal the top card and offer an immediate free cast for a matching creature.
    /// A revealed card that was not cast is put on the bottom.
    RevealSharedTypeCreatureCastOrBottom,
    /// Reveal the top card, put it into hand, then use its mana value for life changes.
    RevealTopHandLoseManaValue {
        gain_life: bool,
    },
    /// Repeat a top-card exile search for nonlands at or above this mana value and deal damage for actual hand arrivals.
    ExileNonlandsToHandAndDamage {
        repeat_at: u16,
    },
    /// The ability source explores using its controller’s library.
    ExploreSource,
    /// The controller may turn one of their face-down creatures face up.
    MayTurnControlledCreatureFaceUp,
    /// Inspect an opponent’s top cards and exile one privately with a cast-only permission.
    LookOpponentExileForCast {
        count: u8,
    },
    /// Return cards in exile linked to the incarnation that produced this death trigger.
    ReturnLinkedSearchCards,
    ReturnRandomLinkedExile,
    OfferVariablePayment {
        each_player: bool,
    },
    DrawEachPlayerForPayments,
    CreateReflexiveGraveyardReturn,
    ReturnGraveyardCreatureWithCorpse(RulesInteger),
    /// An affected player may put up to this many matching cards from hand onto the battlefield.
    PutHandPermanentsOntoBattlefield {
        player: ResolvingPlayer,
        types: BTreeSet<CardType>,
        maximum: u16,
        tapped: bool,
    },
    /// Look at a bounded top segment and put one card into hand without revealing it.
    LookTopTakeOne {
        count: u8,
    },
    /// Search a player's library (and optionally graveyard) and shuffle.
    SearchLibrary {
        filter: Box<LibrarySearch>,
    },
    /// Look at and reorder a single recipient's top cards; no public view is granted.
    Scry {
        player: ResolvingPlayer,
        amount: ValueExpr,
    },
    /// Choose top-card graveyard arrivals, then use physical-owner ordering.
    Surveil {
        player: ResolvingPlayer,
        amount: ValueExpr,
    },
    /// Effect milling only, clamped to the actual library before conversion.
    Mill {
        player: ResolvingPlayer,
        amount: ValueExpr,
    },
    /// Continue at the next instruction when true, otherwise at a strictly
    /// forward absolute destination. The body length denotes completion.
    /// Flip a fair coin; on a loss continue at the specified forward destination.
    FlipCoin {
        lose_destination: usize,
    },
    BranchIfFalse {
        condition: ResolutionCondition,
        destination: usize,
    },
    /// Continue at a strictly forward absolute destination, including body end.
    Jump {
        destination: usize,
    },
    /// Offer Take at the next primitive or Decline at the instruction after it.
    /// Admission rejects a missing next instruction or another control wrapper.
    /// One choice governs the complete following sequence, including any later decisions.
    OptionalSequence {
        chooser: ResolvingPlayer,
        instructions: usize,
    },
    OptionalEffect {
        /// Player who chooses whether the following primitive is taken.
        chooser: ResolvingPlayer,
    },
    /// Offer an exact mana cost to the derived payer during resolution.
    /// Paid and Declined destinations are strictly forward and may equal body end.
    /// A zero cost still requires an explicit Pay acknowledgment at runtime.
    OfferManaPayment {
        /// Player permitted to pay the quoted cost or decline it.
        payer: ResolvingPlayer,
        /// Exact authored cost, validated by the shared mana-cost model.
        cost: ManaCost,
        /// Absolute instruction index selected only by a successful payment.
        paid_destination: usize,
        /// Absolute instruction index selected by an explicit decline.
        declined_destination: usize,
    },
    /// One simultaneous event containing every admitted counted literal group.
    /// Entry replacements and permanent-spell-copy conversion remain separate.
    /// Create a creature token for each opposing creature and fight in corresponding pairs.
    CreateTokensToFight(crate::token::TokenDefinition),
    /// Exact pairs captured by token creation, applied as one noncombat damage event.
    FightPairedCreatures(
        Vec<(
            crate::object_identity::ObjectId,
            crate::object_identity::ObjectId,
        )>,
    ),
    /// Ask opponents in turn order whether to receive the following damage.
    /// The first acceptance proceeds to damage and source sacrifice. No acceptance skips both.
    OfferOpponentsDamage,
    TemptWithTokens {
        creation: crate::token::TokenCreation,
        phase: crate::token::TemptingTokenPhase,
    },
    CreateTokens {
        creations: Vec<crate::token::TokenCreation>,
    },
    CreateAttackingTokens {
        creations: Vec<crate::token::TokenCreation>,
    },
    /// Create one token for the triggered ability's controller, then attach the source
    /// to the new token if legal (CR 702.92, 702.163 and 702.182).
    CreateTokenAndAttach {
        definition: Box<crate::token::TokenDefinition>,
    },
    /// Ordinary direct gain/loss/set for the resolving entry's controller. This
    /// is not damage or payment and does not consume the body's target.
    /// The controller gains life equal to the evaluated resolution-time amount.
    GainControllerLife {
        amount: ValueExpr,
    },
    /// The targeted player loses life and the resolving controller gains it in one event.
    DrainTargetAndGain(ValueExpr),
    AdjustControllerLife {
        adjustment: LifeAdjustment,
    },
    /// Exchange the activated ability's controller's life total with its source's toughness.
    ExchangeLifeWithSourceToughness,
    /// Direct ordinary gain/loss/set for the body's selected player.
    AdjustTargetLife {
        adjustment: LifeAdjustment,
    },
    /// Apply direct life adjustments to each opponent and optionally the controller simultaneously.
    RepeatOpponentLifeLoss,
    OpponentsLoseAndControllerGains(ValueExpr),
    AdjustEachOpponentLife {
        adjustment: LifeAdjustment,
        controller_adjustment: Option<LifeAdjustment>,
    },
    /// Deal noncombat damage to all members of the resolving group simultaneously.
    DamageGroup {
        group: DamageGroup,
        amount: RulesInteger,
    },
    /// Deal one amount simultaneously to creatures controlled by the bound player or planeswalker's controller.
    DamageTargetControllersCreatures {
        amount: ValueExpr,
        include_target: bool,
        require_damaged_creatures_to_attack: bool,
    },
    /// Evaluate the amount once, draw that many, then deal that much to each opponent.
    DrawAndDamageOpponents {
        amount: ValueExpr,
    },
    /// For each player, determine half their life rounded down before dealing damage simultaneously.
    DamagePlayersHalfLife,
    /// Untap all creatures and give the resolving controller control and haste until end of turn.
    UntapAndControlCreatures,
    /// Untap each land controlled by the resolving player.
    UntapControlledLands,
    UntapControlledCreatures,
    TapAttackersAndSkipNextUntap,
    /// Apply a layered effect to the exact permanent that generated this ability.
    ChangeControlledPlaneswalkers {
        parts: ResolvedPartsSpec,
        duration: ResolvedDuration,
    },
    ControlledSubtypeDamagesAttacker(CreatureTypeId),
    /// The ability source deals this damage to the blocker captured by the trigger.
    DamageCapturedBlocker(u16),
    /// The enchanted creature deals the captured damage total to its current or last known controller.
    ReflectAttachedCreatureDamage,
    /// The target deals its power to each other creature its controller currently controls.
    TargetDamagesItsControllersOtherCreatures,
    /// Each member of the captured group deals its power to the original target.
    OtherCreaturesReturnPowerDamage,
    /// Choose a permanent card type and return that whole group from the controller's graveyard.
    ChooseAndReturnOwnGraveyardCardsByType,
    /// Return the complete group of cards with this permanent type to their owner's hand.
    ReturnOwnGraveyardCardsOfTypeToHand(CardType),
    ChangeAbilitySource {
        parts: ResolvedPartsSpec,
        duration: ResolvedDuration,
    },
    /// A targeted player gains control of the ability source indefinitely.
    GiveControlOfAbilitySource,
    /// Proliferate using the current counter-bearing permanents and players.
    Proliferate,
    /// Double each kind of counter on any number of announced permanents.
    DoubleCountersOnTargets,
    /// The controller chooses up to this many counters to remove from a targeted creature.
    RemoveCreatureCounters {
        maximum: u8,
    },
    /// Exile the announced non-Saga nonlands, each bound to a different player, until the source leaves.
    ExileOnePerPlayerUntilSourceLeaves,
    /// The captured combat damage source’s controller draws one card.
    DrawHighestEntrantController,
    DrawCombatDamageSourceController,
    DrawRelatedPlayer,
    /// Each participant names a type, then creatures sharing none of them are destroyed.
    ChooseTypeThenDestroyUnchosen,
    /// Choose the order of artifact, creature and land sacrifice rounds.
    OrderArtifactCreatureLandSacrifices,
    /// A runtime prompt assigned to a participant by a preceding choice instruction.
    ChooseCreatureTypeForPlayer {
        player: crate::priority::SeatId,
        into: CreatureTypeSlot,
    },
    ChooseCreatureType {
        into: CreatureTypeSlot,
        excluding: BTreeSet<CreatureTypeId>,
    },
    /// Deal damage using an amount evaluated on resolution.
    DamageAmount(ValueExpr),
    /// Damage a target; any permanent actually damaged is exiled if it would die this turn.
    DamageAmountThenExileIfDies(ValueExpr),
    /// Deal damage to the targeted creature, and to its current controller if the
    /// resolving player has at least this many instant or sorcery cards in their graveyard.
    DamageCreatureAtSpellMastery {
        amount: ValueExpr,
        minimum: u8,
    },
    DamageFromPower(PowerDamageSource),
    /// The first selected creature deals its power to the second; optionally
    /// divert lethal excess to the second creature's controller if the source has trample.
    CreatureDealsPowerDamageToOther {
        trample_overflow: bool,
    },
    Damage {
        amount: RulesInteger,
    },
    /// Prevent X damage across the controller and their permanents and deal back the amounts prevented.
    CreateReflectingShield,
    CreatePreventionShield {
        amount: RulesInteger,
    },
    CreateDamageMultiplier {
        factor: RulesInteger,
    },
    /// Give the attacking permanent captured by this trigger +1/+1 until end of turn.
    BoostLoneAttacker,
    /// The trigger source gains trample and +X/+X for this turn, using the causing spell mana value.
    BoostSourceByCastManaValue,
    /// Sacrifice the exact source on the unpaid branch of an upkeep keyword.
    SacrificeUpkeepSource(UpkeepKeyword),
    /// The current controller sacrifices the exact evoked permanent.
    SacrificeEvoked,
    /// The controller sacrifices the exact source of the resolving state trigger.
    SacrificeStateTriggerSource,
    /// Resolve an independent fading, vanishing, persist or undying trigger.
    ResolveCounterKeyword(CounterKeyword),
    /// Resolve mentor, evolve or the counters and static keyword suffix of backup.
    ResolveGrowth(GrowthKeyword),
    ResolveTransfer(TransferKeyword),
    /// Resolve the paid life exchanges of extort (CR 702.101).
    ExtortDrain,
    /// Opponents simultaneously lose life and the controller gains the amount actually lost.
    DrainOpponents(ValueExpr),
    /// Counter the opposing stack entry captured by this ward trigger.
    CounterWardSource,
    /// Resolve an unearth activation against the exact graveyard incarnation.
    /// Resolve a revealed hand card's ninjutsu ability (CR 702.49).
    Ninjutsu,
    /// Runtime-only effect of the source-free speed trigger.
    IncreaseSpeed,
    /// Become monarch; a fixed player is reserved for source-free rules triggers.
    BecomeMonarch(Option<crate::priority::SeatId>),
    /// Tap the creature currently enchanted by this ability's source, using last-known attachment if it left.
    TapEnchantedCreature,
    MoveLoyaltyCounterToEnteringPermanent,
    ChooseAttackDirection,
    AdditionalLoyaltyActivation,
    SetBlockDeclarations(BlockDeclarationRule),
    /// Choose a new defending player or planeswalker for a targeted attacker (CR 508.7).
    ReselectAttackDefender,
    RequireAttackSourceNextTurn,
    /// Exile the target until an opponent of this effect's controller becomes monarch.
    ExileUntilNextEndStep,
    ExileTargetUntilOpponentMonarch,
    Unearth,
    /// The controller sacrifices the retained permanents they still control, together.
    SacrificeDelayedObjects(Vec<crate::object_identity::ObjectId>),
    /// Runtime-only delayed ability, bound to the permanent returned by unearth.
    ReturnDelayedExile(Vec<crate::object_identity::ObjectId>),
    ReturnDelayedPermanentToHand(crate::object_identity::ObjectId),
    ScheduleArtifactReturn(crate::object_identity::ObjectId),
    /// Capture a new graveyard incarnation from a preceding zone-change trigger.
    ScheduleGraveyardReturnAtEndStep(DelayedGraveyardReturn),
    ReturnDelayedGraveyardCard(crate::object_identity::ObjectId),
    ReturnDelayedGraveyardCardTransformed(crate::object_identity::ObjectId),
    ReturnSacrificedCardToBattlefield,
    ReturnDyingSourceTapped,
    /// Return the bound creature card under its owner’s control as a face-down Forest.
    ReturnDyingCreatureAsForest,
    ExileUnearthed(crate::object_identity::ObjectId),
    /// Runtime-only delayed effect of a specific dash, blitz or warp cast.
    EndStepCast {
        kind: EndStepCast,
        permanent: crate::object_identity::ObjectId,
        turn: u64,
    },
    /// Delayed CR 702.88 opportunity for one exact card exiled on resolution.
    ReboundCast(crate::object_identity::ObjectId),
    CastTargetGraveyardInstant,
    /// Grant current graveyard cards flashback at their own mana costs through cleanup.
    GrantFlashback(FlashbackSubject),
    /// Cast the exiled source for its madness price, or put it into its owner's graveyard.
    MadnessCast,
    MiracleCast(crate::object_identity::ObjectId),
    /// Exile and optionally cast the qualifying card of this self-cast trigger.
    Cascade,
    /// Randomly bottom the remaining cards of this same cascade instance.
    CascadeBottom,
    /// Resolve one independently stacked, nontargeted printed keyword trigger.
    ResolveKeywordTrigger(CombatTriggerKeyword),
    /// Destroy the targeted permanent if it can be destroyed.
    DestroyTarget,
    /// Destroy the permanent captured as the damaging source, with no targeting.
    DestroyCapturedDamageSource,
    ExileCombatDamagedCreature,
    /// Destroy the exact combatant captured by a blocker or combat damage trigger.
    DestroyCapturedCombatCreature {
        no_regeneration: bool,
    },
    /// Attach the source Equipment to target creature you control on resolution.
    Attach(crate::attachment::AttachAbility),
    /// Unattach the source of the resolving Reconfigure ability.
    Unattach,
    /// Optionally unattach one Equipment from a controlled creature, then tap and protect it.
    UnattachEquipmentAndProtectHost,
    /// Put this many +1/+1 counters on the original source, if it remains on the battlefield.
    Adapt(u16),
    PutCountersOnSource {
        amount: RulesInteger,
    },
    /// The untargeted resolving half of crew or saddle.
    ResolvePowerAbility(crate::activated::PowerAbility),
    /// An attacking trigger source gets this bonus and keyword until end of turn.
    BoostSaddledAttacker {
        adjustment: PowerToughness,
        keyword: KeywordAbility,
    },
    ReturnToOwnersHand,
    /// Return the targeted creature card in the controller's graveyard under that player's control.
    ReturnOwnGraveyardCreatureToBattlefield,
    /// Return a targeted permanent card from the resolving controller's graveyard.
    ReturnOwnGraveyardPermanentToBattlefield {
        tapped: bool,
    },
    /// Return a targeted creature or planeswalker card from any graveyard.
    ReturnAnyGraveyardCreatureOrPlaneswalker,
    /// Return a creature card from any graveyard and give it the specified subtype and black color.
    ReturnAnyGraveyardCreatureBlackSubtype(CreatureTypeId),
    /// Exile the exact targeted battlefield incarnation.
    ExileTarget,
    /// Exile the exact permanent that generated this resolving ability.
    ExileAbilitySource,
    /// Exile every physical card currently in any graveyard.
    /// Place a +1/+1 counter on this ability’s source. On the specified resolution
    /// this turn, remove all of those counters and deal their number to each creature and player.
    AddCounterAndDischargeOnResolution(u8),
    ExileAllGraveyards,
    /// Compare the target creature with the revealed cost card when this resolves.
    ExileCreatureAtMostRevealedPower,
    /// Exile all battlefield creatures and planeswalkers below the revealed card’s mana value.
    ExileCreaturesAndPlaneswalkersBelowRevealedManaValue,
    /// Move the controller’s graveyard into their library, then shuffle it.
    /// Put all commanders owned by the ability controller in the command zone or their graveyard into hand.
    ReturnOwnedCommandersToHand,
    ShuffleOwnGraveyardIntoLibrary,
    /// Shuffle the legal cards of the bounded secondary clause into the target player's library.
    ShuffleTargetGraveyardCards {
        maximum: u8,
    },
    ShuffleEachHandAndGraveyard,
    EndControllerTurn,
    ShuffleOwnLibrary,
    ExileTopForFreePlay,
    /// Exile the top card, offer to put the exiled permanent onto the battlefield, and repeat only on entry.
    ExileTopRepeatOnPermanentEntry,
    /// Exile cards from the targeted opponent's library until one nonland is exiled.
    /// The controller may cast that exact nonland without its mana cost this turn.
    ExileTargetUntilNonlandForFreeCast,
    /// Return the exact activated source from its graveyard.
    ReturnSourceGraveyardToBattlefield {
        tapped: bool,
    },
    /// Return the exact triggered source from its graveyard to its owner’s hand.
    ReturnSourceGraveyardToHand,
    /// Optionally return a creature card from your graveyard to your hand.
    ReturnUpToOneOwnGraveyardCreatureToHand,
    /// Move the announced creature card from your graveyard into your hand.
    ReturnTargetGraveyardCreatureToHand,
    /// Sample distinct qualifying cards from the controller's graveyard, then return them together.
    ReturnRandomGraveyardCards {
        filter: Option<Box<ObjectFilter>>,
        count: std::num::NonZeroU8,
        to_battlefield: bool,
    },
    /// Reveal the target opponent's hand, then choose a qualifying card to move.
    ChooseRevealedHandCard {
        minimum_mana_value: u16,
        nonland_only: bool,
        count: std::num::NonZeroU8,
        action: ChosenHandAction,
    },
    ReturnUpToOneRecentGraveyardCard,
    /// Exile up to two announced graveyard card targets; gain life for creatures actually exiled.
    ExileUpToTwoGraveyardTargetsGainLife,
    /// Exile the target until the triggering source's current incarnation leaves.
    ExileTargetUntilSourceLeaves,
    ImprintTargetLand,
    ImprintTargetCreature,
    /// Exile a creature and retain each exiled card as a donor for the linked activation grant.
    ExileTargetAsAbilityDonor,
    /// Use the last known controller and power of the permanent this body exiled.
    ExiledTargetControllerGainsPower,
    RememberPermanentTarget,
    ControllerLosesRememberedManaValue,
    CounterSpell,
    /// Counter a spell and put its physical card in exile.
    CounterSpellToExile,
    CopySpell,
    /// Put a copy of the targeted stack ability under the resolving controller.
    CopyAbility,
    /// Capture the donor's represented copiable definition for this activated
    /// ability's exact battlefield source. Entry/token/static copies and other
    /// copy exceptions are outside this instruction's admitted language.
    CopyTargetToSource {
        duration: ResolvedDuration,
        power_toughness: Option<PowerToughness>,
    },
    /// Capture a resolving amount and give this target temporary power and keywords.
    /// Give the target creature −X/−X through end of turn using its announced X.
    WeakenTargetByChosenX,
    BoostTargetPower {
        amount: ValueExpr,
        also_toughness: bool,
        keywords: BTreeSet<crate::characteristics::KeywordAbility>,
    },
    CreateContinuousEffect {
        parts: ResolvedPartsSpec,
        duration: ResolvedDuration,
    },
    /// Give a permanent creature type, a subtype, counted power/toughness and keywords.
    AnimateTargetAsCountedCreature {
        amount: ValueExpr,
        subtype: CreatureTypeId,
        keywords: BTreeSet<KeywordAbility>,
        duration: ResolvedDuration,
    },
    /// Put an indestructible counter on the targeted permanent.
    PutIndestructibleCounter,
    /// Put one loyalty counter on the legal targeted planeswalker.
    PutLoyaltyCounter,
    /// Choose a color as this resolves and grant the target protection through cleanup.
    GrantProtectionChosenColor(ProtectionRecipients),
    PutPowerToughnessCounters {
        counters: PowerToughnessCounters,
    },
    /// Draw from the target player’s library one card at a time.
    DrawTargetPlayer {
        amount: ValueExpr,
    },
    /// Both players draw in active-player, nonactive-player order.
    DrawControllerAndTarget {
        amount: ValueExpr,
    },
    /// Draw and lose life equal to creature cards in the target player's graveyard.
    DrawAndLoseLifeForTargetGraveyardCreatures,
    /// Draw for each player in active-player turn order.
    DrawEachPlayer {
        amount: ValueExpr,
    },
    /// Each player controlling a permanent with this literal name draws a card.
    DrawPlayersControllingName(spellwake_contracts::names::EnglishCardName),
    /// Reads the resolving stack entry's controller, without consuming a target.
    DrawController {
        amount: ValueExpr,
    },
    /// Each affected player chooses matching permanents they control to return to their owners' hands.
    ReturnControlledPermanents {
        players: AffectedPlayers,
        amount: ValueExpr,
        filter: PermanentTypeFilter,
    },
    /// Choose a creature you control other than the ability source, then return or sacrifice it.
    ChooseOtherControlledCreature {
        sacrifice: bool,
        or_exile_source: bool,
    },
    /// Each player exiles a combined quota of their hand cards and controlled permanents.
    ExileFromHandOrBattlefield(ValueExpr),
    /// The captured player exiles a card from hand. The ability controller can play it while exiled.
    ExileRelatedHandCardForPlay,
    /// Each player may pay the spell's X life per chosen creature to save it from sacrifice.
    PayLifeOrSacrificeCreatures,
    ExileChosenPermanents {
        players: AffectedPlayers,
        amount: ValueExpr,
        filter: PermanentTypeFilter,
    },
    SacrificeColoredPermanents,
    /// Each player chooses one of their creatures to receive a vow counter.
    ChooseVowedCreatures,
    ChooseCreaturesToKeep,
    DestroyOtherCreatures,
    /// Sacrifice every creature its controller did not choose in the preceding instruction.
    SacrificeOtherCreatures,
    /// The controller chooses zero to the stated number of permanents at resolution, without targeting.
    /// Choose named artifact and creature tokens with pairwise disjoint names and copy them.
    CopyDifferentlyNamedTokens,
    /// Permanently copy the creature this Aura entered attached to across the current nontoken cohort.
    CopyOtherControlledNontokensAsEntryHost,
    /// Attach all controlled Equipment, or any number of controlled Auras and Equipment, to the ability source.
    AttachControlledToSource {
        optional_auras_and_equipment: bool,
    },
    /// The controller may attach any eligible battlefield Auras to the source.
    AttachBattlefieldAurasToSource,
    /// The controller may put their eligible hand and graveyard Auras onto the source.
    EnterHandOrGraveyardAurasAttachedToSource,
    /// Attach this Equipment to the entering object, or the entering Equipment to the target.
    AttachOnEntry {
        entering_equipment: bool,
    },
    UntapUpTo {
        kind: CardType,
        maximum: u16,
    },
    Bolster(u16),
    /// Create one base Army token if no Army permanent is controlled by the resolving player.
    CreateArmyIfAbsent {
        army: CreatureTypeId,
        subtype: CreatureTypeId,
    },
    /// Choose a controlled Army, place +1/+1 counters and add the amass subtype.
    Amass {
        army: CreatureTypeId,
        subtype: CreatureTypeId,
        amount: u16,
    },
    /// Each player mills the top cards of their own library simultaneously.
    MillEachPlayer(u16),
    PutLibraryCohortIntoGraveyard(LibraryCohort),
    /// Distinct permanent targets are announced together and checked individually as resolution begins.
    AffectPermanentTargets {
        minimum: u16,
        maximum: u16,
        action: PermanentTargetAction,
    },
    /// One continuous effect on any number of announced, still-legal permanents.
    ModifyAnyNumberOfPermanents {
        parts: ResolvedPartsSpec,
        duration: ResolvedDuration,
    },
    /// Each affected player chooses as many eligible controlled permanents as
    /// possible, then the complete selection is sacrificed simultaneously.
    /// This is an effect, not an additional cost or an optional-effect receipt.
    Sacrifice {
        /// Native resolving roles or all current participants.
        players: AffectedPlayers,
        /// Evaluated once under the resolving controller, then clamped per domain.
        amount: ValueExpr,
        /// Evaluated types are distinct from physical Battlefield/control checks.
        filter: PermanentTypeFilter,
    },
    /// Each affected player chooses as many physical cards from their own hand
    /// as possible, then discards the complete selection simultaneously.
    /// Random discard and choosing for another player are separate instructions.
    DiscardUpTo {
        maximum: u16,
    },
    /// The ability controller discards one uniformly selected hand card.
    DiscardOneAtRandom(ResolvingPlayer),
    /// The targeted player sacrifices a random permanent and repeats for each nonland sacrificed.
    SacrificeRandomUntilLand,
    /// Sacrifice one controlled permanent matching the filter. Skip the remainder if none was sacrificed.
    SacrificeToContinue {
        filter: PermanentTypeFilter,
    },
    SacrificeMatching(SacrificeSelection),
    Discard {
        /// Native resolving roles or all current participants.
        players: AffectedPlayers,
        /// Evaluated once under the resolving controller, then clamped per hand.
        amount: ValueExpr,
    },
}
impl Instruction {
    /// Whether an instruction in a chosen mode requires that mode's target to be legal.
    pub(crate) fn uses_mode_target(&self) -> bool {
        use ResolvingPlayer::{ExiledTargetController, TargetPlayer, TargetSpellController};
        if let Self::CreateTokens { creations } = self {
            return !creations.is_empty()
                && creations.iter().all(|creation| {
                    matches!(
                        creation.recipients,
                        crate::token::TokenRecipients::ExiledTargetController
                            | crate::token::TokenRecipients::RememberedTargetController
                    )
                });
        }
        if let Self::SearchLibrary { filter } = self {
            return filter.source == SearchSource::TargetOpponentLibrary
                || filter.curse_host() == Some(AuraSearchHost::TargetPlayer);
        }
        matches!(
            self,
            Self::PhaseOut(PhasingSubject::Target)
                | Self::AttachOnEntry {
                    entering_equipment: true
                }
                | Self::RequireAttackSourceNextTurn
                | Self::DrawAndLoseLifeForTargetGraveyardCreatures
                | Self::ReturnTargetGraveyardCreatureToHand
                | Self::ChooseRevealedHandCard { .. }
                | Self::GiveControlOfAbilitySource
                | Self::RememberPermanentTarget
                | Self::RemoveCreatureCounters { .. }
                | Self::ControllerLosesRememberedManaValue
                | Self::RegenerateTarget
                | Self::GoadTarget
                | Self::TargetDamagesItsControllersOtherCreatures
                | Self::OtherCreaturesReturnPowerDamage
                | Self::RequireTargetAttack
                | Self::RemoveTargetFromCombat
                | Self::TapTargetWhileControllingSource
                | Self::DrawControllerAndTarget { .. }
                | Self::DrawTargetPlayer { .. }
                | Self::DamageFromPower(_)
                | Self::CreatureDealsPowerDamageToOther { .. }
                | Self::DamageAmount(_)
                | Self::DamageAmountThenExileIfDies(_)
                | Self::DamageCreatureAtSpellMastery { .. }
                | Self::Damage { .. }
                | Self::AdjustTargetLife { .. }
                | Self::DamageTargetControllersCreatures { .. }
                | Self::DrainTargetAndGain(_)
                | Self::DestroyTarget
                | Self::DestroyTargetAndAttachedEquipment
                | Self::ExileCreatureAtMostRevealedPower
                | Self::ExileTarget
                | Self::ExileTargetWithLink(_)
                | Self::ImprintTargetLand
                | Self::ImprintTargetCreature
                | Self::ExileTargetAsAbilityDonor
                | Self::ExileTargetUntilSourceLeaves
                | Self::ExileUntilNextEndStep
                | Self::ExileTargetUntilOpponentMonarch
                | Self::DamageGroup {
                    group: DamageGroup::ControllerAndTargetCreature,
                    ..
                }
                | Self::ExileRelatedPermanents(_)
                | Self::ChooseNewTargets
                | Self::GainControlOfSpell
                | Self::ChangeOnlyTarget
                | Self::LoseLifeEqualToTargetSpellManaValue
                | Self::ReturnToOwnersHand
                | Self::CounterSpell
                | Self::CounterSpellToExile
                | Self::CopySpell
                | Self::Attach(_)
                | Self::WatchTargetCombatDamageDraw
                | Self::GrantProtectionChosenColor(ProtectionRecipients::TargetPermanent)
                | Self::PutIndestructibleCounter
                | Self::PutLoyaltyCounter
                | Self::PutPowerToughnessCounters { .. }
                | Self::AnimateTargetAsCountedCreature { .. }
                | Self::CreateContinuousEffect { .. }
                | Self::WeakenTargetByChosenX
                | Self::BoostTargetPower { .. }
                | Self::BindOrderedPermanents { .. }
                | Self::ExileOrderedPermanent(_)
                | Self::ExileLibraryTop {
                    player: TargetPlayer,
                    ..
                }
                | Self::PutTargetOwnedExiledLandsOntoBattlefield
                | Self::LookOpponentExileForCast { .. }
                | Self::ExileFromTargetLibrary
                | Self::CreatePreventionShield { .. }
                | Self::CreateReflectingShield
                | Self::PreventDamageThisTurn(
                    crate::damage::TurnPreventionFilter::ToTarget
                        | crate::damage::TurnPreventionFilter::CombatFromCreaturesOtherThanTarget
                )
                | Self::ExileForBlackToken { .. }
                | Self::ExileForSubtypeToken(_)
                | Self::CastDefeatedSiege
                | Self::CastTargetGraveyardInstant
                | Self::ReturnGraveyardCreatureWithSubtypeCounters { .. }
                | Self::ReturnAnyGraveyardCreatureBlackSubtype(_)
                | Self::ReturnOwnGraveyardCreatureToBattlefield
                | Self::ReturnOwnGraveyardPermanentToBattlefield { .. }
                | Self::ReturnAnyGraveyardCreatureOrPlaneswalker
                | Self::ExiledTargetControllerGainsPower
                | Self::OfferManaPayment {
                    payer: TargetPlayer | TargetSpellController | ExiledTargetController,
                    ..
                }
                | Self::OptionalSequence {
                    chooser: TargetPlayer | TargetSpellController | ExiledTargetController,
                    ..
                }
                | Self::OptionalEffect {
                    chooser: TargetPlayer | TargetSpellController | ExiledTargetController
                }
                | Self::ManifestTop {
                    player: TargetPlayer | TargetSpellController | ExiledTargetController,
                    ..
                }
                | Self::Scry {
                    player: TargetPlayer | TargetSpellController,
                    ..
                }
                | Self::Surveil {
                    player: TargetPlayer | TargetSpellController,
                    ..
                }
                | Self::Mill {
                    player: TargetPlayer | TargetSpellController,
                    ..
                }
                | Self::ExileCardsFromGraveyard {
                    player: TargetPlayer,
                    ..
                }
                | Self::GrantFlashback(FlashbackSubject::Target)
                | Self::DiscardOneAtRandom(TargetPlayer)
                | Self::Discard {
                    players: AffectedPlayers::One(TargetPlayer | TargetSpellController),
                    ..
                }
                | Self::SacrificeRandomUntilLand
                | Self::ExileTargetUntilNonlandForFreeCast
                | Self::Sacrifice {
                    players: AffectedPlayers::One(TargetPlayer | TargetSpellController),
                    ..
                }
        ) || matches!(self, Self::DrawController { amount } if amount.is_target_player_tapped_creatures())
    }
    /// Whether this operation can be the single effect of an optional wrapper.
    /// Admission and runtime availability share this exhaustive language boundary.
    pub(crate) fn is_optional_primitive(&self) -> bool {
        match self {
            Self::ChooseAndReturnOwnGraveyardCardsByType
            | Self::ReturnOwnGraveyardCardsOfTypeToHand(_)
            | Self::TargetDamagesItsControllersOtherCreatures
            | Self::OtherCreaturesReturnPowerDamage
            | Self::ReflectAttachedCreatureDamage
            | Self::DestroyAttachedEnchantmentSubtype(_)
            | Self::GrantFlashback(_)
            | Self::TransformSubtype(_)
            | Self::CastLinkedSpellMatchingX
            | Self::DestroyDamagedPlayersNonlandsAtX
            | Self::PutChosenXChargeCounters
            | Self::DestroyNonlandsAtSourceChargeCount
            | Self::AddCounterAndDischargeOnResolution(_)
            | Self::ReturnOwnedCommandersToHand
            | Self::ExileTopForFreePlay
            | Self::ExileTargetUntilNonlandForFreeCast
            | Self::ExileTopRepeatOnPermanentEntry
            | Self::ShuffleEachHandAndGraveyard
            | Self::EndControllerTurn
            | Self::ShuffleOwnLibrary
            | Self::ShuffleOwnGraveyardIntoLibrary
            | Self::ShuffleTargetGraveyardCards { .. }
            | Self::ExileCreaturesAndPlaneswalkersBelowRevealedManaValue
            | Self::ExileAllGraveyards
            | Self::FixedTargetClause(_)
            | Self::ExileResolvingSpell
            | Self::DestroyFixedTargets
            | Self::DestroyTargetAndAttachedEquipment
            | Self::ReturnFixedTargets
            | Self::FightSourceWithTarget
            | Self::FightFixedTargets
            | Self::ExileUntilOtherTargetLeaves
            | Self::CopyOtherTargetAsArtifact
            | Self::ReturnChosenXGraveyardCreatures
            | Self::ReturnGraveyardTargetsTapped(_)
            | Self::ReturnCapturedDyingCreature
            | Self::ReturnCapturedDyingCreatureWithTypeAndColor { .. }
            | Self::ReturnGraveyardCreatureWithSubtypeCounters { .. }
            | Self::ReturnGraveyardCohort(_)
            | Self::DivideThreeDamageAmongTargets
            | Self::DistributePlusOneCounters { .. }
            | Self::DamageOrderedTargets { .. } => false,
            Self::PutHandPermanentsOntoBattlefield { .. }
            | Self::MayTurnControlledCreatureFaceUp
            | Self::ExploreSource
            | Self::MayRevealLibraryTop
            | Self::ManifestTop { .. }
            | Self::ExileNonlandsToHandAndDamage { .. }
            | Self::RevealTopHandLoseManaValue { .. }
            | Self::RevealSharedTypeCreatureCastOrBottom
            | Self::RevealOddCastOrDraw
            | Self::RevealUntilLandKind
            | Self::RevealOpponentChoosesCreature
            | Self::ShuffleAbilitySourceIntoLibrary
            | Self::RevealToNameAfterSourceShuffle(_)
            | Self::WatchSmallCreatureCombatDraw(_)
            | Self::ChooseOwnCommanderToCast
            | Self::ChooseMatchingHandSpellToCast
            | Self::SearchEachLibraryForFreeCast
            | Self::ChooseFoundSpell
            | Self::CastFoundSpell(_)
            | Self::LookTopOneRestGraveyard { .. }
            | Self::RevealTopHandRestGraveyard { .. }
            | Self::LookTopOneRestBottom { .. }
            | Self::RevealTopCreaturesAndLandsRestBottom { .. }
            | Self::RevealHandAndBottomPiles
            | Self::RevealDividePiles
            | Self::DrawToChosenOpponentHandSize
            | Self::LookTopPlaneswalkers
            | Self::RevealTopPermanentOrHand
            | Self::LookTopLandBattlefieldTapped
            | Self::LookTopReveal { .. }
            | Self::LookTopTakeOne { .. } => false,
            Self::ExileForBlackToken { .. }
            | Self::LoseLifeAndScryForSubtype(_)
            | Self::ReturnDelayedGraveyardToHand(_)
            | Self::CopyCreatureExiledForAbility
            | Self::DamageFromGrantingPermanent { .. }
            | Self::CopyEntrantAsArtifact
            | Self::ReplaceSourceCreatedTokens
            | Self::AbilitySourceControllerLosesLife(_) => false,
            Self::ReturnDyingCreatureAsForest
            | Self::GiveEquippedCreatureToUpkeepPlayer
            | Self::CopyEntrantOnceEachTurn
            | Self::ReturnDelayedGraveyardCard(_)
            | Self::ReturnDelayedGraveyardCardTransformed(_)
            | Self::CopyResolvingSpell
            | Self::SacrificeCombatContributor
            | Self::MoveLoyaltyCounterToEnteringPermanent
            | Self::Mill { .. }
            | Self::RelatedPlayerLosesLife(_)
            | Self::PutQuestCounterOnSource
            | Self::ExileOrderedPermanent(_)
            | Self::DamageGroup { .. }
            | Self::DamageTargetControllersCreatures { .. }
            | Self::AttachOnEntry { .. }
            | Self::PayControllerLife(_)
            | Self::OfferVariablePayment { .. }
            | Self::DrawEachPlayerForPayments
            | Self::CreateReflexiveGraveyardReturn
            | Self::ReturnGraveyardCreatureWithCorpse(_)
            | Self::ReturnRandomLinkedExile
            | Self::ReturnLinkedSearchCards
            | Self::ReturnSourceGraveyardToBattlefield { .. }
            | Self::ReturnSourceGraveyardToHand => true,
            Self::DrawAndLoseLifeForTargetGraveyardCreatures
            | Self::ReturnAnyGraveyardCreatureBlackSubtype(_)
            | Self::ReturnUpToOneRecentGraveyardCard
            | Self::ReturnTargetGraveyardCreatureToHand
            | Self::ReturnRandomGraveyardCards { .. }
            | Self::ChooseRevealedHandCard { .. }
            | Self::ReturnUpToOneOwnGraveyardCreatureToHand
            | Self::ExileUpToTwoGraveyardTargetsGainLife => false,
            Self::ExileCapturedGraveyardCard
            | Self::GrantGraveyardCastAndWatchAbilities
            | Self::GainCastCardAbilities { .. }
            | Self::ExileCardsFromGraveyard { .. } => false,
            Self::SearchLibrary { filter } if filter.supports_optional_wrapper() => true,
            Self::SacrificeMatching(_) => true,
            Self::Discard {
                players: AffectedPlayers::One(ResolvingPlayer::Controller),
                amount: ValueExpr::Literal(amount),
            } if amount == &1.into() => true,
            Self::PutTargetOwnedExiledLandsOntoBattlefield
            | Self::BindOrderedPermanents { .. }
            | Self::ExileLibraryTop { .. }
            | Self::LookOpponentExileForCast { .. }
            | Self::ExileFromTargetLibrary
            | Self::SearchLibrary { .. }
            | Self::Scry { .. }
            | Self::Surveil { .. }
            | Self::FlipCoin { .. }
            | Self::BranchIfFalse { .. }
            | Self::Jump { .. }
            | Self::DrainOpponents(_)
            | Self::ExtortDrain
            | Self::ResolveGrowth(_)
            | Self::ResolveCounterKeyword(_)
            | Self::SacrificeEvoked
            | Self::SacrificeStateTriggerSource
            | Self::SacrificeUpkeepSource(_)
            | Self::OfferSourceCounters { .. }
            | Self::OptionalSequence { .. }
            | Self::OptionalEffect { .. }
            | Self::OfferManaPayment { .. }
            | Self::ModifyAnyNumberOfPermanents { .. }
            | Self::AffectPermanentTargets {
                action:
                    PermanentTargetAction::TapAndSkipNextUntap
                    | PermanentTargetAction::Untap
                    | PermanentTargetAction::Destroy
                    | PermanentTargetAction::ReturnToHand
                    | PermanentTargetAction::PutPlusOneCounter
                    | PermanentTargetAction::PutPlusOneCounterOnOthers
                    | PermanentTargetAction::PutCountersByCurrentType
                    | PermanentTargetAction::Modify { .. }
                    | PermanentTargetAction::ModifyWithPartner { .. },
                ..
            }
            | Self::MillEachPlayer(_)
            | Self::PutLibraryCohortIntoGraveyard(_)
            | Self::Bolster(_)
            | Self::CreateArmyIfAbsent { .. }
            | Self::Amass { .. }
            | Self::UntapUpTo { .. }
            | Self::ChooseVowedCreatures
            | Self::ChooseCreaturesToKeep
            | Self::DestroyOtherCreatures
            | Self::SacrificeOtherCreatures
            | Self::SacrificeColoredPermanents
            | Self::ExileChosenPermanents { .. }
            | Self::ExileFromHandOrBattlefield(_)
            | Self::ExileRelatedHandCardForPlay
            | Self::PayLifeOrSacrificeCreatures
            | Self::ReturnControlledPermanents { .. }
            | Self::ChooseOtherControlledCreature { .. }
            | Self::Sacrifice { .. }
            | Self::DiscardOneAtRandom(_)
            | Self::SacrificeRandomUntilLand
            | Self::DiscardUpTo { .. }
            | Self::SacrificeToContinue { .. }
            | Self::Discard { .. }
            | Self::RepeatOpponentLifeLoss
            | Self::OpponentsLoseAndControllerGains(_)
            | Self::AdjustEachOpponentLife { .. }
            | Self::RememberPermanentTarget
            | Self::ControllerLosesRememberedManaValue
            | Self::ExiledTargetControllerGainsPower
            | Self::ChooseNewTargets
            | Self::GainControlOfSpell
            | Self::ChangeOnlyTarget
            | Self::LoseLifeEqualToTargetSpellManaValue
            | Self::GoadOtherPlayersCreatures
            | Self::AdditionalCombat { .. }
            | Self::AdditionalCombatAfterUntappingAttackers
            | Self::TakeExtraTurn
            | Self::AddMana(_)
            | Self::AbandonSourceScheme
            | Self::TabletopHonorAction { .. }
            | Self::CreateNonlegendaryLoyaltyCopy
            | Self::WeakenOpposingAttackersUntilNextTurn
            | Self::PhaseOtherPlaneswalkersNextEndStep
            | Self::ChooseMeldPartner(_)
            | Self::ExileMeldPair
            | Self::MeldExiledPair(_)
            | Self::PairWithCreature { .. }
            | Self::PhaseOut(_)
            | Self::PhaseOutObjects(_)
            | Self::ReturnOneNonlandPerOpponent
            | Self::WeakenDelayedAttacker(_)
            | Self::DrawRevealRemoveLoyalty
            | Self::CreateReflexiveDiscardDamage
            | Self::RemoveLoyaltyExileTopForPlay
            | Self::DamageDamagedPlayersCreatures
            | Self::DamageOpponentsAmount(_)
            | Self::DamageRelatedPlayerAmount(_)
            | Self::ImpulseCastOrDamage(_)
            | Self::ExileMilledCreatures { .. }
            | Self::VoteForGraveyardCards { .. }
            | Self::ShuffleOnePermanentPerPlayer
            | Self::RevealCreatureOfSacrificedType
            | Self::ExileNonlandForFormerControllers
            | Self::ReturnExiledTargetCohort { .. }
            | Self::ReturnCardsDestroyedThisWay
            | Self::RevealCreatureForEachTargetController
            | Self::ReturnOddGraveyardPermanentIfPaid { .. }
            | Self::PutGraveyardCreatureAtMostX
            | Self::PutLandFromExiledCohort
            | Self::ExileTopForEachSpellType(_)
            | Self::ExileTopFreeNoncreatureSpells(_)
            | Self::ExileRelatedTopForCastThisTurn
            | Self::ExileRelatedTopForPlay
            | Self::ExileTopForPlay { .. }
            | Self::ExileTopFaceDown
            | Self::ScheduleCreatedTokenExile(_)
            | Self::ScheduleCreatedTokenSacrifice
            | Self::ExileDelayedObjects(_)
            | Self::ExileRelatedPermanents(_)
            | Self::ReturnCardsExiledWithLink(_)
            | Self::TapTargetWhileControllingSource
            | Self::RegisterTokenLinkedExile
            | Self::ReturnTokenLinkedExile
            | Self::CreateEmblem(_)
            | Self::CreateOpponentEmblems(_)
            | Self::PreventDamageThisTurn(_)
            | Self::MoveGroup { .. }
            | Self::PutTargetInLibrary(_)
            | Self::PutOwnHandCardOnLibraryBottom
            | Self::PutGroupOnLibraryBottom(_)
            | Self::ExileGraveyardTarget
            | Self::PutEquipmentFromHandOrGraveyard
            | Self::EquipNewToken
            | Self::Encore
            | Self::GrantNewTokensHaste
            | Self::PutCounterOnRelatedPermanent
            | Self::PutCountersOnControlledCreatures
            | Self::PutCountersOnControlledSubtype(_)
            | Self::PutCountersOnControlledCreaturesAndGrant(_)
            | Self::PutCountersOnOtherCreatures
            | Self::PutCreatureAndOtherPlaneswalkerCounters
            | Self::PutChargeCounterOnSource
            | Self::PutBlessingCounterOnSource
            | Self::PutNamedCounterOnSource { .. }
            | Self::PutStorageCounterOnSource
            | Self::LinkedPlayerLosesDamageAmount
            | Self::RelatedPlayerLosesHalfLife
            | Self::ExileDefeatedSiege
            | Self::ExileSourceAndGainLife(_)
            | Self::ExileAbilitySource
            | Self::ReturnSourceToHand
            | Self::UntapAbilitySource
            | Self::TapAbilitySource
            | Self::RegenerateSource
            | Self::RegenerateControlledSubtype(_)
            | Self::DestroyGroupWithoutRegeneration(_)
            | Self::DestroyGroup(_)
            | Self::ExileOnePerPlayerUntilSourceLeaves
            | Self::DoubleCountersOnTargets
            | Self::ExileCombatDamagedCreature
            | Self::DestroyCapturedCombatCreature { .. }
            | Self::DestroyCapturedDamageSource
            | Self::RemoveCreatureCounters { .. }
            | Self::UntapControlledLands
            | Self::UntapControlledCreatures
            | Self::TapAttackersAndSkipNextUntap
            | Self::ControlledSubtypeDamagesAttacker(_)
            | Self::RevealTopAndRemoveAttackerIfForest
            | Self::DamageCapturedBlocker(_)
            | Self::ChangeControlledPlaneswalkers { .. }
            | Self::UntapAndControlCreatures
            | Self::Proliferate
            | Self::AttachControlledToSource { .. }
            | Self::AttachBattlefieldAurasToSource
            | Self::EnterHandOrGraveyardAurasAttachedToSource
            | Self::CopyCreatureAsToken
            | Self::CopyDifferentlyNamedTokens
            | Self::CopyOtherControlledNontokensAsEntryHost
            | Self::DamagePlayersHalfLife
            | Self::DrawAndDamageOpponents { .. } => false,
            Self::ResolveTransfer(_)
            | Self::CreateTokenAndAttach { .. }
            | Self::CreateTokensToFight(_)
            | Self::FightPairedCreatures(_)
            | Self::OfferOpponentsDamage
            | Self::TemptWithTokens { .. }
            | Self::CreateTokens { .. }
            | Self::CreateAttackingTokens { .. }
            | Self::GainControllerLife { .. }
            | Self::DrainTargetAndGain(_)
            | Self::AdjustControllerLife { .. }
            | Self::ExchangeLifeWithSourceToughness
            | Self::AdjustTargetLife { .. }
            | Self::ChooseTypeThenDestroyUnchosen
            | Self::OrderArtifactCreatureLandSacrifices
            | Self::ChooseCreatureTypeForPlayer { .. }
            | Self::ChooseCreatureType { .. }
            | Self::DamageFromPower(_)
            | Self::CreatureDealsPowerDamageToOther { .. }
            | Self::DamageAmount(_)
            | Self::DamageAmountThenExileIfDies(_)
            | Self::DamageCreatureAtSpellMastery { .. }
            | Self::Damage { .. }
            | Self::CreatePreventionShield { .. }
            | Self::CreateReflectingShield
            | Self::CreateDamageMultiplier { .. }
            | Self::ResolveKeywordTrigger(_)
            | Self::IncreaseSpeed
            | Self::RequireCondition(_)
            | Self::RequireAttackSourceNextTurn
            | Self::ChooseAttackDirection
            | Self::ReselectAttackDefender
            | Self::SetBlockDeclarations(_)
            | Self::AdditionalLoyaltyActivation
            | Self::Ninjutsu
            | Self::Unearth
            | Self::Cascade
            | Self::CascadeBottom
            | Self::Myriad
            | Self::CopyForOtherCreatures
            | Self::WatchNextSpellOrLoyaltyTwice
            | Self::WatchNextLoyaltyCopy
            | Self::ChoosePlaneswalkerTypeForCopies
            | Self::CopyCapturedAbility
            | Self::CopyOnCast(_)
            | Self::MadnessCast
            | Self::MiracleCast(_)
            | Self::CastDefeatedSiege
            | Self::CastTargetGraveyardInstant
            | Self::ReboundCast(_)
            | Self::EndStepCast { .. }
            | Self::SacrificeDelayedObjects(_)
            | Self::ScheduleArtifactReturn(_)
            | Self::ScheduleGraveyardReturnAtEndStep(_)
            | Self::ReturnSacrificedCardToBattlefield
            | Self::ReturnDyingSourceTapped
            | Self::ReturnDelayedPermanentToHand(_)
            | Self::ReturnDelayedExileWithCounters(_)
            | Self::ReturnDelayedExile(_)
            | Self::ExileUnearthed(_)
            | Self::CounterWardSource
            | Self::BoostSourceByCastManaValue
            | Self::BoostLoneAttacker
            | Self::BoostSaddledAttacker { .. }
            | Self::SetControlledCreatureSizeToHand
            | Self::ModifyGroupByAmount { .. }
            | Self::AddExperienceCounter
            | Self::ControlManaSourceThroughNextTurn
            | Self::GrantControllerFlashUntilCleanup
            | Self::RequireOpponentsCreaturesBlockThisTurn
            | Self::RequireBlocksAgainstTargetThisTurn
            | Self::SuppressGroupKeywords { .. }
            | Self::ModifyGroupUntilEndOfTurn { .. }
            | Self::BoostSource { .. }
            | Self::ResolvePowerAbility(_)
            | Self::Adapt(_)
            | Self::TransformSource
            | Self::TransformSourceAndAttachToTarget
            | Self::ClearSourceCounters(_)
            | Self::PutCountersOnSource { .. }
            | Self::UnattachEquipmentAndProtectHost
            | Self::Unattach
            | Self::Attach(_)
            | Self::RegenerateTarget
            | Self::GoadTarget
            | Self::RequireTargetAttack
            | Self::RemoveTargetFromCombat
            | Self::DestroyTarget
            | Self::ExileCreatureAtMostRevealedPower
            | Self::ExileTarget
            | Self::ExileTargetWithLink(_)
            | Self::ImprintTargetLand
            | Self::ImprintTargetCreature
            | Self::ExileTargetAsAbilityDonor
            | Self::ExileTargetUntilSourceLeaves
            | Self::ExileUntilNextEndStep
            | Self::ExileTargetUntilOpponentMonarch
            | Self::ReturnToOwnersHand
            | Self::ExileForSubtypeToken(_)
            | Self::ReturnOwnGraveyardCreatureToBattlefield
            | Self::ReturnOwnGraveyardPermanentToBattlefield { .. }
            | Self::ReturnAnyGraveyardCreatureOrPlaneswalker
            | Self::CounterSpell
            | Self::CounterSpellToExile
            | Self::CopyAbility
            | Self::CopySpell
            | Self::CopyTargetToSource { .. }
            | Self::AffectPermanentTargets {
                action:
                    PermanentTargetAction::Exile
                    | PermanentTargetAction::Tap
                    | PermanentTargetAction::ToggleTap,
                ..
            }
            | Self::ChangeAbilitySource { .. }
            | Self::GiveControlOfAbilitySource
            | Self::CreateContinuousEffect { .. }
            | Self::WeakenTargetByChosenX
            | Self::BoostTargetPower { .. }
            | Self::AnimateTargetAsCountedCreature { .. }
            | Self::WatchTargetCombatDamageDraw
            | Self::GrantProtectionChosenColor(_)
            | Self::PutIndestructibleCounter
            | Self::PutLoyaltyCounter
            | Self::PutPowerToughnessCounters { .. }
            | Self::DrawHighestEntrantController
            | Self::DrawRelatedPlayer
            | Self::DrawCombatDamageSourceController
            | Self::DrawControllerAndTarget { .. }
            | Self::DrawTargetPlayer { .. }
            | Self::DrawEachPlayer { .. }
            | Self::DrawPlayersControllingName(_)
            | Self::DrawController { .. }
            | Self::BecomeMonarch(_)
            | Self::TapEnchantedCreature
            | Self::LookTopCreatureSharingAttachedSubtype { .. }
            | Self::DivideTargetPermanentsAndSacrificePile => true,
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
}
impl SplitCard {
    pub fn half(&self, half: SplitHalf) -> &Program {
        match half {
            SplitHalf::Left => &self.left,
            SplitHalf::Right => &self.right,
        }
    }

    /// Combine the two names, mana costs and spell types outside the stack (CR 709.4).
    pub fn into_program(self) -> Option<Program> {
        let ProgramKind::Spell(left) = &self.left.kind else {
            return None;
        };
        let ProgramKind::Spell(right) = &self.right.kind else {
            return None;
        };
        let mut combined = self.left.clone();
        let ProgramKind::Spell(spell) = &mut combined.kind else {
            return None;
        };
        spell.cost.generic = left.cost.generic.plus(&right.cost.generic);
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
        combined.x_mana_symbols = self
            .left
            .x_mana_symbols
            .checked_add(self.right.x_mana_symbols)?;
        combined.split = Some(Box::new(self));
        Some(combined)
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

/// The body's admission gate is independent of its instruction order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BodyTarget {
    /// A card in the resolving controller’s graveyard.
    SingleOwnGraveyardCard,
    FilteredOwnGraveyardCard(ObjectFilter),
    /// Creature or planeswalker card in any graveyard.
    AnyGraveyardCreatureOrPlaneswalker,
    /// Any physical card in any graveyard.
    AnyGraveyardCard,
    /// A face-up exiled card owned by this ability or spell's controller that has flashback.
    OwnedExiledFlashbackCard,
    RecentlyDiscardedOrFromLibrary,
    /// Any creature card in any graveyard.
    AnyGraveyardCreature,
    FilteredPermanent(ObjectFilter),
    None,
    SinglePermanent,
    SingleCreature,
    SinglePlayer,
    /// A current opponent of the controller of this spell or ability.
    SingleOpponent,
    /// Deliberately excludes planeswalkers and battles until those are modeled.
    SingleCreatureOrPlayer,
    /// A player or a battlefield planeswalker.
    SinglePlayerOrPlaneswalker,
    /// One opponent or planeswalker, then up to one creature that player controls.
    OpponentOrPlaneswalkerThenCreature,
    /// A battlefield planeswalker, evaluated at announcement and resolution.
    SinglePlaneswalker,
    /// Any creature, planeswalker or player (battles are not yet modeled).
    SingleAnyTarget,
    /// Any current spell or ability on the stack.
    SingleStackEntry,
    /// A spell or ability on the stack targeting a permanent you control.
    StackEntryTargetingControlledPermanent,
    ControlledAbility {
        colorless_source: bool,
    },
    SingleSpell(SpellFilter),
}
impl BodyTarget {
    fn guarantees_creature(self) -> bool {
        matches!(self, Self::SingleCreature)
            || matches!(self, Self::FilteredPermanent(filter) if filter.guarantees_creature())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutableBody {
    pub target: BodyTarget,
    pub instructions: Vec<Instruction>,
    pub(crate) triggered_modes: Option<TriggeredModes>,
    graveyard_target_bounds: Option<(usize, usize)>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DependentTargets {
    CreatureControlledByPrimary,
    PrimaryPlayersGraveyard { maximum: u8 },
}

impl DependentTargets {
    pub(crate) fn maximum(self) -> usize {
        match self {
            Self::CreatureControlledByPrimary => 1,
            Self::PrimaryPlayersGraveyard { maximum } => usize::from(maximum),
        }
    }
}

/// Modal abilities choose the printed modes and their targets during trigger placement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TriggeredModes {
    pub minimum: u8,
    pub maximum: u8,
    pub distinct_targets: bool,
    pub options: Vec<ExecutableBody>,
}
#[derive(Clone, Copy)]
enum OperandFamily {
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
        let Self::FilteredPermanent(filter) = self else {
            return self;
        };
        Self::FilteredPermanent(filter.with_ability_source(source))
    }
}

impl ExecutableBody {
    pub(crate) fn dependent_targets(&self) -> Option<DependentTargets> {
        match (self.target, self.instructions.first()) {
            (BodyTarget::OpponentOrPlaneswalkerThenCreature, _) => {
                Some(DependentTargets::CreatureControlledByPrimary)
            }
            (
                BodyTarget::SinglePlayer,
                Some(Instruction::ShuffleTargetGraveyardCards { maximum }),
            ) => Some(DependentTargets::PrimaryPlayersGraveyard { maximum: *maximum }),
            _ => None,
        }
    }

    /// Bounds of the one printed variable-cardinality target clause, if present.
    pub fn ordered_any_target_bounds(&self) -> Option<(usize, usize)> {
        match self.instructions.as_slice() {
            [Instruction::DamageOrderedTargets { .. }]
                if self.target == BodyTarget::OpponentOrPlaneswalkerThenCreature =>
            {
                Some((1, 2))
            }
            [
                Instruction::DamageOrderedTargets { .. }
                | Instruction::DivideThreeDamageAmongTargets,
            ] if self.target == BodyTarget::SingleAnyTarget => Some((1, 3)),
            _ => None,
        }
    }
    /// Total amount that must be assigned in positive shares while choosing targets.
    pub fn target_division_total(&self) -> Option<u16> {
        match self.instructions.as_slice() {
            [Instruction::DistributePlusOneCounters { total }] => Some(*total),
            _ => None,
        }
    }

    pub fn permanent_target_bounds(&self) -> Option<(usize, usize)> {
        self.graveyard_target_bounds
            .or_else(|| self.inferred_permanent_target_bounds())
    }

    fn inferred_permanent_target_bounds(&self) -> Option<(usize, usize)> {
        if let Some(total) = self.target_division_total() {
            return Some((1, usize::from(total)));
        }
        if self.target == BodyTarget::None {
            return None;
        }
        match self.instructions.as_slice() {
            [Instruction::BindOrderedPermanents { minimum, maximum }, ..] => {
                Some((usize::from(*minimum), usize::from(*maximum)))
            }
            [
                Instruction::ExileForBlackToken {
                    add_types: true, ..
                },
            ] if self.target
                == BodyTarget::FilteredOwnGraveyardCard(ObjectFilter::enchantment().non_aura()) =>
            {
                Some((0, 1))
            }
            [Instruction::ReturnChosenXGraveyardCreatures]
                if self.target
                    == BodyTarget::FilteredOwnGraveyardCard(ObjectFilter::creature()) =>
            {
                Some((0, usize::MAX))
            }
            [
                Instruction::SacrificeToContinue { .. },
                Instruction::ReturnGraveyardTargetsTapped(count),
            ] if self.target == BodyTarget::FilteredOwnGraveyardCard(ObjectFilter::creature()) => {
                Some((usize::from(*count), usize::from(*count)))
            }
            [Instruction::ReturnUpToOneRecentGraveyardCard]
                if self.target == BodyTarget::RecentlyDiscardedOrFromLibrary =>
            {
                Some((0, 1))
            }
            [Instruction::ExileUpToTwoGraveyardTargetsGainLife]
                if self.target == BodyTarget::AnyGraveyardCard =>
            {
                Some((0, 2))
            }
            [Instruction::PhaseOtherPlaneswalkersNextEndStep] => Some((0, 2)),
            [
                Instruction::ShuffleOnePermanentPerPlayer,
                Instruction::ExileNonlandForFormerControllers,
            ] => Some((1, usize::MAX)),
            [
                Instruction::ReturnOneNonlandPerOpponent
                | Instruction::DoubleCountersOnTargets
                | Instruction::ExileOnePerPlayerUntilSourceLeaves,
            ] => Some((0, usize::MAX)),
            [Instruction::ModifyAnyNumberOfPermanents { .. }] => Some((0, usize::MAX)),
            [
                Instruction::AffectPermanentTargets {
                    minimum: 0,
                    maximum: u16::MAX,
                    action: PermanentTargetAction::Exile,
                },
                Instruction::ReturnExiledTargetCohort {
                    next_end_step: true,
                    ..
                },
            ] => Some((0, usize::MAX)),
            [
                Instruction::OptionalSequence {
                    chooser: ResolvingPlayer::Controller,
                    instructions: 2,
                },
                Instruction::AffectPermanentTargets {
                    minimum,
                    maximum,
                    action: PermanentTargetAction::Exile,
                },
                Instruction::ReturnExiledTargetCohort { .. },
            ] => Some((usize::from(*minimum), usize::from(*maximum))),
            [
                Instruction::AffectPermanentTargets {
                    minimum,
                    maximum,
                    action:
                        PermanentTargetAction::TapAndSkipNextUntap
                        | PermanentTargetAction::Tap
                        | PermanentTargetAction::Untap,
                },
                Instruction::DrawController { .. }
                | Instruction::Scry {
                    player: ResolvingPlayer::Controller,
                    ..
                }
                | Instruction::Surveil {
                    player: ResolvingPlayer::Controller,
                    ..
                }
                | Instruction::AdjustControllerLife { .. }
                | Instruction::GainControllerLife { .. },
            ] => Some((usize::from(*minimum), usize::from(*maximum))),
            [
                Instruction::OfferManaPayment { .. },
                Instruction::AffectPermanentTargets {
                    minimum,
                    maximum,
                    action: PermanentTargetAction::TapAndSkipNextUntap,
                },
            ]
            | [
                Instruction::AffectPermanentTargets {
                    minimum,
                    maximum,
                    action: PermanentTargetAction::Exile,
                },
                Instruction::RevealCreatureForEachTargetController
                | Instruction::ReturnExiledTargetCohort { .. }
                | Instruction::ReturnOddGraveyardPermanentIfPaid { .. },
                ..,
            ]
            | [
                Instruction::AffectPermanentTargets {
                    minimum, maximum, ..
                },
            ]
            | [
                Instruction::AffectPermanentTargets {
                    minimum,
                    maximum,
                    action: PermanentTargetAction::Destroy,
                },
                Instruction::ReturnCardsDestroyedThisWay,
            ] => Some((usize::from(*minimum), usize::from(*maximum))),
            _ => None,
        }
    }

    pub fn new(target: BodyTarget, instructions: Vec<Instruction>) -> Self {
        Self {
            target,
            instructions,
            triggered_modes: None,
            graveyard_target_bounds: None,
        }
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

    /// Check every scalar and operand against the explicit whole-body contract.
    ///
    /// # Errors
    /// Returns a definition error for invalid instructions, operands, resolving
    /// player contexts, costs, forward destinations or local-slot assignments.
    /// Trigger-dependent amounts and related players require a trigger binding.
    pub fn validate(&self, vocabulary: &CreatureTypeVocabulary) -> Result<(), DefinitionError> {
        self.validate_in(vocabulary, BodyContext::Unbound)
    }

    fn validate_in(
        &self,
        vocabulary: &CreatureTypeVocabulary,
        context: BodyContext<'_>,
    ) -> Result<(), DefinitionError> {
        if let Some((minimum, maximum)) = self.graveyard_target_bounds
            && (minimum > maximum
                || maximum == 0
                || self.inferred_permanent_target_bounds().is_some()
                || !matches!(self.target, BodyTarget::FilteredOwnGraveyardCard(_))
                || !matches!(
                    self.instructions.as_slice(),
                    [Instruction::ReturnTargetGraveyardCreatureToHand]
                        | [
                            Instruction::PutTargetInLibrary(_),
                            Instruction::DrawController { .. }
                        ]
                ))
        {
            return Err(DefinitionError::InvalidInstruction);
        }
        if let Some(modes) = &self.triggered_modes {
            if !matches!(context, BodyContext::TriggerBound { .. })
                || self.target != BodyTarget::None
                || !self.instructions.is_empty()
                || modes.options.len() < 2
                || modes.options.len() > usize::from(u8::MAX)
                || (modes.distinct_targets && modes.minimum > 1)
                || modes.maximum < modes.minimum
                || usize::from(modes.maximum) > modes.options.len()
            {
                return Err(DefinitionError::InvalidInstruction);
            }
            for option in &modes.options {
                if option.triggered_modes.is_some()
                    || option.instructions.is_empty()
                    || option.ordered_any_target_bounds().is_some()
                    || option
                        .instructions
                        .iter()
                        .any(|op| matches!(op, Instruction::FixedTargetClause(_)))
                {
                    return Err(DefinitionError::InvalidInstruction);
                }
                option.validate_in(vocabulary, context)?;
            }
            return Ok(());
        }
        if let BodyTarget::FilteredPermanent(filter)
        | BodyTarget::FilteredOwnGraveyardCard(filter) = self.target
        {
            filter.validate_vocabulary(vocabulary)?;
        }
        if let BodyTarget::FilteredPermanent(filter) = self.target {
            if filter.has_bound_exclusion() {
                return Err(DefinitionError::InvalidInstruction);
            }
            if filter.is_creature_or_other_planeswalker() {
                if !matches!(context, BodyContext::Activated { .. }) {
                    return Err(DefinitionError::InvalidInstruction);
                }
            } else if filter.has_unbound_trigger_exclusion()
                && !matches!(context, BodyContext::TriggerBound { .. })
                && !(matches!(context, BodyContext::Activated { .. })
                    && self.target
                        == BodyTarget::FilteredPermanent(
                            ObjectFilter::planeswalker()
                                .controlled()
                                .other_than_trigger_source(),
                        )
                    && self.instructions == [Instruction::PhaseOtherPlaneswalkersNextEndStep])
            {
                return Err(DefinitionError::InvalidInstruction);
            }
            if filter.requires_same_name_as_source()
                && !matches!(context, BodyContext::Activated { .. })
            {
                return Err(DefinitionError::InvalidInstruction);
            }
            if (filter.has_related_player()
                && !matches!(context, BodyContext::TriggerBound { condition, grouping: TriggerGrouping::EachOccurrence | TriggerGrouping::OneOrMoreByPlayer } if condition.captures_related_player()))
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
        incoming[0] = Some(FlowFacts::default());
        // Unreachable instructions still validate scalar/context/operand syntax.
        // Their local reads do not contribute facts or make a target reachable.
        let declared_slots: BTreeSet<_> = self
            .instructions
            .iter()
            .filter_map(|op| {
                if let Instruction::ChooseCreatureType { into, .. } = op {
                    Some(*into)
                } else {
                    None
                }
            })
            .collect();
        for (index, instruction) in self.instructions.iter().enumerate() {
            let reachable = incoming[index].is_some();
            let mut facts = incoming[index].take().unwrap_or_else(|| FlowFacts {
                definitely: declared_slots.clone(),
                ..FlowFacts::default()
            });
            facts.exact_creature_target = self.target.guarantees_creature()
                && self
                    .permanent_target_bounds()
                    .is_none_or(|bounds| bounds == (1, 1));
            let mut defined_slots = facts.definitely.clone();
            let mut donor_moved = facts.donor_moved;
            if let Instruction::ChooseCreatureType { into, .. } = instruction {
                if reachable && facts.possibly.contains(into) {
                    return Err(DefinitionError::DuplicateCreatureTypeSlot);
                }
                // Unreachable code has no incoming assignment history.
                if !reachable {
                    defined_slots.remove(into);
                }
            }
            let exclusion = match instruction {
                Instruction::LookTopReveal {
                    filter: LookTopFilter::AllOfChosenType(slot),
                    ..
                } => Some(*slot),
                Instruction::MoveGroup { group, .. }
                | Instruction::DestroyGroup(group)
                | Instruction::DestroyGroupWithoutRegeneration(group)
                | Instruction::PutGroupOnLibraryBottom(group) => group.chosen_type_slot(),
                Instruction::ModifyGroupByAmount { group, .. }
                | Instruction::ModifyGroupUntilEndOfTurn { group, .. }
                | Instruction::SuppressGroupKeywords { group, .. } => group.chosen_type_slot(),
                _ => None,
            };
            if exclusion.is_some_and(|slot| !defined_slots.contains(&slot)) {
                return Err(DefinitionError::UndefinedCreatureTypeSlot);
            }
            let family = match instruction {
                Instruction::BranchIfFalse {
                    condition,
                    destination,
                } => {
                    condition.validate(context, vocabulary)?;
                    if !facts.moved_library_cards
                        && [&condition.left, &condition.right].iter().any(|value| {
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
                    if !facts.checked_destruction_target
                        && [&condition.left, &condition.right].iter().any(|value| {
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
                    if !facts.definitely_discarded
                        && [&condition.left, &condition.right].iter().any(|value| {
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
                    if !facts.definitely_selected_graveyard_exile
                        && [&condition.left, &condition.right].contains(&&ValueExpr::event(
                            EventValueKind::SelectedGraveyardExileCreatures,
                        ))
                    {
                        return Err(DefinitionError::InvalidInstruction);
                    }
                    if !facts.definitely_sacrificed
                        && [&condition.left, &condition.right]
                            .contains(&&ValueExpr::event(EventValueKind::LastSacrificeSucceeded))
                    {
                        return Err(DefinitionError::InvalidInstruction);
                    }
                    if !facts.definitely_checked_top
                        && [&condition.left, &condition.right].iter().any(|value| {
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
                    if [&condition.left, &condition.right].iter().any(|value| {
                        matches!(
                            value,
                            ValueExpr::ObjectStat {
                                subject: ValueObjectRef::TargetObject,
                                stat: ObjectStatKind::ManaValue,
                            }
                        )
                    }) && !matches!(self.target, BodyTarget::FilteredOwnGraveyardCard(_))
                    {
                        return Err(DefinitionError::InvalidInstruction);
                    }
                    Self::validate_destination(index, *destination, length)?;
                    if [&condition.left, &condition.right].iter().any(|value| {
                        matches!(
                            value,
                            ValueExpr::ObjectMatches {
                                subject: MatchObjectRef::TargetObject,
                                ..
                            }
                        )
                    }) {
                        if !facts.exact_creature_target || donor_moved {
                            return Err(DefinitionError::OperandFamily);
                        }
                        Some(OperandFamily::Permanent)
                    } else {
                        None
                    }
                }
                Instruction::FlipCoin {
                    lose_destination: destination,
                }
                | Instruction::Jump { destination } => {
                    Self::validate_destination(index, *destination, length)?;
                    None
                }
                Instruction::OfferSourceCounters {
                    amount,
                    placed_destination,
                    declined_destination,
                } => {
                    if amount < &0.into() || !matches!(context, BodyContext::TriggerBound { .. }) {
                        return Err(DefinitionError::InvalidInstruction);
                    }
                    Self::validate_destination(index, *placed_destination, length)?;
                    Self::validate_destination(index, *declined_destination, length)?;
                    None
                }
                Instruction::OptionalEffect { chooser }
                | Instruction::OptionalSequence { chooser, .. } => {
                    let next = index
                        .checked_add(1)
                        .ok_or(DefinitionError::InvalidControlFlow)?;
                    let effect = self
                        .instructions
                        .get(next)
                        .ok_or(DefinitionError::MissingOptionalEffect)?;
                    if !effect.is_optional_primitive() {
                        return Err(DefinitionError::UnsupportedOptionalEffect);
                    }
                    let span = match instruction {
                        Instruction::OptionalSequence { instructions, .. }
                            if *instructions >= 2 =>
                        {
                            *instructions
                        }
                        Instruction::OptionalSequence { .. } => {
                            return Err(DefinitionError::InvalidControlFlow);
                        }
                        _ => 1,
                    };
                    let declined = next
                        .checked_add(span)
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
                Instruction::OfferManaPayment {
                    payer,
                    cost,
                    paid_destination,
                    declined_destination,
                } => {
                    let family =
                        payer.validate(context, self.target, facts.definitely_exiled_target)?;
                    if !cost.valid() {
                        return Err(DefinitionError::InvalidResolutionPaymentCost);
                    }
                    Self::validate_destination(index, *paid_destination, length)?;
                    Self::validate_destination(index, *declined_destination, length)?;
                    family
                }
                Instruction::CreateTokenAndAttach { definition } => {
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
                Instruction::CreateTokensToFight(definition) => {
                    if self.target != BodyTarget::None
                        || self.instructions.len() != 1
                        || !definition.program().has_type(CardType::Creature)
                        || definition
                            .program()
                            .permanent()
                            .is_none_or(|permanent| permanent.attachment.is_some())
                    {
                        return Err(DefinitionError::InvalidInstruction);
                    }
                    definition
                        .validate(vocabulary)
                        .map_err(|_| DefinitionError::TokenDefinition)?;
                    None
                }
                Instruction::FightPairedCreatures(_) => {
                    return Err(DefinitionError::InvalidInstruction);
                }
                Instruction::TemptWithTokens { creation, phase } => {
                    use crate::token::{TemptingTokenPhase, TokenRecipients};
                    if *phase != TemptingTokenPhase::Initial
                        || self.target != BodyTarget::None
                        || self.instructions.len() != 1
                        || creation.recipients != TokenRecipients::Controller
                        || creation.base_size.is_some()
                    {
                        return Err(DefinitionError::InvalidInstruction);
                    }
                    creation
                        .definition
                        .validate(vocabulary)
                        .map_err(|_| DefinitionError::TokenDefinition)?;
                    creation
                        .amount
                        .validate_amount(&defined_slots, context, &facts, vocabulary)?;
                    None
                }
                Instruction::CreateTokens { creations }
                | Instruction::CreateAttackingTokens { creations } => {
                    if matches!(instruction, Instruction::CreateAttackingTokens { .. })
                        && (self.target != BodyTarget::None
                            || creations.iter().any(|creation| {
                                !creation.enters_tapped
                                    || creation.recipients
                                        != crate::token::TokenRecipients::Controller
                            }))
                    {
                        return Err(DefinitionError::InvalidInstruction);
                    }
                    if creations.is_empty() {
                        return Err(DefinitionError::InvalidInstruction);
                    }
                    for creation in creations {
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
                                crate::token::TokenSize::AtCreation(amount) => amount
                                    .validate_amount(&defined_slots, context, &facts, vocabulary)?,
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
                    }
                    None
                }
                Instruction::DrainTargetAndGain(amount) => {
                    amount.validate_amount(&defined_slots, context, &facts, vocabulary)?;
                    Some(OperandFamily::Player)
                }
                Instruction::GainControllerLife { amount } => {
                    amount.validate_amount(&defined_slots, context, &facts, vocabulary)?;
                    if matches!(
                        amount,
                        ValueExpr::ObjectStat {
                            subject: ValueObjectRef::TargetObject,
                            stat: ObjectStatKind::Toughness
                        }
                    ) {
                        if !self.target.guarantees_creature() {
                            return Err(DefinitionError::OperandFamily);
                        }
                        Some(OperandFamily::Permanent)
                    } else {
                        None
                    }
                }
                Instruction::AdjustControllerLife { adjustment } => {
                    if !adjustment.valid() {
                        return Err(DefinitionError::InvalidInstruction);
                    }
                    None
                }
                Instruction::ExchangeLifeWithSourceToughness => {
                    if self.target != BodyTarget::None
                        || !matches!(context, BodyContext::Activated { .. })
                    {
                        return Err(DefinitionError::InvalidInstruction);
                    }
                    None
                }
                Instruction::AdjustTargetLife { adjustment } => {
                    if !adjustment.valid() {
                        return Err(DefinitionError::InvalidInstruction);
                    }
                    Some(OperandFamily::Player)
                }
                Instruction::RepeatOpponentLifeLoss => None,
                Instruction::OpponentsLoseAndControllerGains(amount) => {
                    amount.validate_amount(&defined_slots, context, &facts, vocabulary)?;
                    None
                }
                Instruction::AdjustEachOpponentLife {
                    adjustment,
                    controller_adjustment,
                } => {
                    if !adjustment.valid()
                        || controller_adjustment
                            .as_ref()
                            .is_some_and(|value| !value.valid())
                    {
                        return Err(DefinitionError::InvalidInstruction);
                    }
                    None
                }
                Instruction::RememberPermanentTarget => {
                    if index != 0 {
                        return Err(DefinitionError::InvalidInstruction);
                    }
                    facts.remembered_permanent = true;
                    Some(OperandFamily::Permanent)
                }
                Instruction::ControllerLosesRememberedManaValue => {
                    if !facts.remembered_permanent {
                        return Err(DefinitionError::InvalidInstruction);
                    }
                    None
                }
                Instruction::ExiledTargetControllerGainsPower => {
                    if !facts.definitely_exiled_target || self.target != BodyTarget::SingleCreature
                    {
                        return Err(DefinitionError::InvalidInstruction);
                    }
                    None
                }
                Instruction::AdditionalCombat { untap } => {
                    let admitted = match untap {
                        CombatUntapGroup::ControlledCreatures => self.target == BodyTarget::None,
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
                Instruction::AdditionalCombatAfterUntappingAttackers => {
                    if !matches!(context, BodyContext::TriggerBound { condition, .. } if *condition == TriggerSpec::self_attacks())
                    {
                        return Err(DefinitionError::InvalidInstruction);
                    }
                    None
                }
                Instruction::TransformSubtype(kind) => {
                    if self.target != BodyTarget::None || vocabulary.label(*kind).is_none() {
                        return Err(DefinitionError::InvalidInstruction);
                    }
                    None
                }
                Instruction::PreventDamageThisTurn(
                    crate::damage::TurnPreventionFilter::CombatFromCreaturesWithoutEitherSubtype(
                        first,
                        second,
                    ),
                ) if first == second
                    || vocabulary.label(*first).is_none()
                    || vocabulary.label(*second).is_none() =>
                {
                    return Err(DefinitionError::InvalidInstruction);
                }
                Instruction::PreventDamageThisTurn(
                    crate::damage::TurnPreventionFilter::CombatFromCreaturesWithoutSubtype(subtype),
                ) if vocabulary.label(*subtype).is_none() => {
                    return Err(DefinitionError::InvalidInstruction);
                }
                Instruction::PreventDamageThisTurn(
                    crate::damage::TurnPreventionFilter::ToTarget
                    | crate::damage::TurnPreventionFilter::CombatFromCreaturesOtherThanTarget,
                ) => Some(OperandFamily::Permanent),
                Instruction::TakeExtraTurn | Instruction::PreventDamageThisTurn(_) => None,
                Instruction::PutOwnHandCardOnLibraryBottom => {
                    if self.target != BodyTarget::None {
                        return Err(DefinitionError::OperandFamily);
                    }
                    None
                }
                Instruction::PutTargetInLibrary(_) => match self.target {
                    BodyTarget::SingleCreature
                    | BodyTarget::SinglePermanent
                    | BodyTarget::FilteredPermanent(_) => Some(OperandFamily::Permanent),
                    BodyTarget::AnyGraveyardCard | BodyTarget::FilteredOwnGraveyardCard(_) => {
                        Some(OperandFamily::GraveyardCard)
                    }
                    _ => return Err(DefinitionError::OperandFamily),
                },
                Instruction::ExileGraveyardTarget
                    if matches!(
                        self.target,
                        BodyTarget::AnyGraveyardCard | BodyTarget::AnyGraveyardCreature
                    ) =>
                {
                    Some(OperandFamily::GraveyardCard)
                }
                Instruction::ExileGraveyardTarget => return Err(DefinitionError::OperandFamily),
                Instruction::GrantGraveyardCastAndWatchAbilities
                    if self.target == BodyTarget::AnyGraveyardCreature
                        && matches!(context, BodyContext::Activated { .. }) =>
                {
                    Some(OperandFamily::GraveyardCard)
                }
                Instruction::GrantGraveyardCastAndWatchAbilities
                | Instruction::GainCastCardAbilities { .. } => {
                    return Err(DefinitionError::InvalidInstruction);
                }
                Instruction::ExileCapturedGraveyardCard => {
                    if self.target != BodyTarget::None
                        || !matches!(context, BodyContext::TriggerBound {
                        condition,
                        grouping: TriggerGrouping::EachOccurrence,
                    } if *condition == TriggerSpec::self_dies()
                        || matches!(condition.event, TriggerEvent::PutIntoGraveyard { from_library: true, .. }))
                    {
                        return Err(DefinitionError::InvalidInstruction);
                    }
                    None
                }
                Instruction::ExileCardsFromGraveyard { player, .. } => {
                    if !matches!(
                        player,
                        ResolvingPlayer::Controller
                            | ResolvingPlayer::TargetPlayer
                            | ResolvingPlayer::UniqueRelatedPlayer
                    ) {
                        return Err(DefinitionError::InvalidInstruction);
                    }
                    facts.definitely_selected_graveyard_exile = true;
                    player.validate(context, self.target, facts.definitely_exiled_target)?
                }
                Instruction::PutGroupOnLibraryBottom(group) => {
                    if self.target != BodyTarget::None || group.uses_outside_named_slots() {
                        return Err(DefinitionError::InvalidInstruction);
                    }
                    group.validate_vocabulary(vocabulary)?;
                    donor_moved = true;
                    None
                }
                Instruction::MoveGroup {
                    destination:
                        PublicZoneDestination::Command
                        | PublicZoneDestination::Graveyard
                        | PublicZoneDestination::Battlefield
                        | PublicZoneDestination::Library,
                    ..
                } => return Err(DefinitionError::InvalidInstruction),
                Instruction::MoveGroup { group, .. }
                | Instruction::DestroyGroupWithoutRegeneration(group)
                | Instruction::DestroyGroup(group) => {
                    if group.uses_outside_named_slots() {
                        return Err(DefinitionError::InvalidInstruction);
                    }
                    group
                        .validate_vocabulary(vocabulary)
                        .map_err(|_| DefinitionError::InvalidInstruction)?;
                    facts.definitely_destroyed |=
                        !matches!(instruction, Instruction::MoveGroup { .. });
                    donor_moved = true;
                    None
                }
                Instruction::DrawAndLoseLifeForTargetGraveyardCreatures
                    if self.target == BodyTarget::SinglePlayer =>
                {
                    Some(OperandFamily::Player)
                }
                Instruction::DrawAndLoseLifeForTargetGraveyardCreatures => {
                    return Err(DefinitionError::InvalidInstruction);
                }
                Instruction::DrawRelatedPlayer => {
                    ResolvingPlayer::UniqueRelatedPlayer.validate(context, self.target, false)?
                }
                Instruction::DrawHighestEntrantController => {
                    ResolvingPlayer::HighestEntrantController.validate(
                        context,
                        self.target,
                        false,
                    )?
                }
                Instruction::DrawCombatDamageSourceController => {
                    ResolvingPlayer::CombatDamageSourceController.validate(
                        context,
                        self.target,
                        false,
                    )?
                }
                Instruction::ExileOnePerPlayerUntilSourceLeaves => {
                    if !matches!(self.target, BodyTarget::FilteredPermanent(_))
                        || !matches!(context, BodyContext::TriggerBound { .. })
                    {
                        return Err(DefinitionError::InvalidInstruction);
                    }
                    Some(OperandFamily::Permanent)
                }
                Instruction::RemoveCreatureCounters { maximum } => {
                    if *maximum == 0 || self.target != BodyTarget::SingleCreature {
                        return Err(DefinitionError::InvalidInstruction);
                    }
                    Some(OperandFamily::Permanent)
                }
                Instruction::DestroyCapturedCombatCreature { .. } => {
                    if self.target != BodyTarget::None
                        || !matches!(
                            context,
                            BodyContext::TriggerBound {
                                condition: TriggerSpec {
                                    event: TriggerEvent::DealsDamage {
                                        source,
                                        recipient: DamageTriggerRecipient::Creature,
                                        combat_only: true,
                                    },
                                    ..
                                },
                                grouping: TriggerGrouping::EachOccurrence,
                            } if *source == ObjectFilter::self_object()
                        ) && !matches!(
                            context,
                            BodyContext::TriggerBound {
                                condition: TriggerSpec {
                                    event: TriggerEvent::BlocksOrBlocked {
                                        subject,
                                        kind: BlockTriggerKind::BlocksOrBlockedByEach { required_subtype: Some(_) },
                                    },
                                    ..
                                },
                                grouping: TriggerGrouping::EachOccurrence,
                            } if *subject == ObjectFilter::any().attached_to_self()
                        )
                    {
                        return Err(DefinitionError::InvalidInstruction);
                    }
                    None
                }
                Instruction::ExileCombatDamagedCreature => {
                    if self.target != BodyTarget::None
                        || !matches!(
                            context,
                            BodyContext::TriggerBound {
                                condition: TriggerSpec {
                                    event: TriggerEvent::DealsDamage {
                                        source,
                                        recipient: DamageTriggerRecipient::Creature,
                                        combat_only: true,
                                    },
                                    ..
                                },
                                grouping: TriggerGrouping::EachOccurrence,
                            } if *source == ObjectFilter::self_object()
                        )
                    {
                        return Err(DefinitionError::InvalidInstruction);
                    }
                    None
                }
                Instruction::DestroyCapturedDamageSource => {
                    if self.target != BodyTarget::None
                        || !matches!(
                            context,
                            BodyContext::TriggerBound {
                                condition: TriggerSpec {
                                    event: TriggerEvent::DealsDamage {
                                        source: ObjectFilter::Clause(c),
                                        recipient: DamageTriggerRecipient::Controller,
                                        combat_only: false,
                                    },
                                    ..
                                },
                                grouping: TriggerGrouping::EachOccurrence,
                            } if matches!(c.subtypes, SubtypeFilter::Has(_))
                        )
                    {
                        return Err(DefinitionError::InvalidInstruction);
                    }
                    None
                }
                Instruction::DoubleCountersOnTargets => {
                    if self.target != BodyTarget::SinglePermanent {
                        return Err(DefinitionError::InvalidInstruction);
                    }
                    Some(OperandFamily::Permanent)
                }
                Instruction::AttachOnEntry { entering_equipment } => {
                    let (relation, operand) = if *entering_equipment {
                        (
                            ObjectFilter::any()
                                .controlled()
                                .with_artifact_subtype(ArtifactSubtype::Equipment)
                                .or_self(),
                            Some(OperandFamily::Permanent),
                        )
                    } else {
                        (ObjectFilter::creature().controlled().legendary(), None)
                    };
                    if !matches!(
                        context,
                        BodyContext::TriggerBound {
                            condition,
                            grouping: TriggerGrouping::EachOccurrence,
                        } if *condition == TriggerSpec::enters(relation)
                    ) || self.target
                        != if *entering_equipment {
                            BodyTarget::FilteredPermanent(ObjectFilter::creature().controlled())
                        } else {
                            BodyTarget::None
                        }
                    {
                        return Err(DefinitionError::InvalidInstruction);
                    }
                    operand
                }
                Instruction::DamageCapturedBlocker(amount) => {
                    if *amount == 0
                        || self.target != BodyTarget::None
                        || !matches!(
                            context,
                            BodyContext::TriggerBound {
                                condition: TriggerSpec {
                                    event: TriggerEvent::BlocksOrBlocked {
                                        subject,
                                        kind:
                                            BlockTriggerKind::BlockedByEach { without_flanking_only: false }
                                            | BlockTriggerKind::BlocksOrBlockedByEach { required_subtype: None },
                                    },
                                    ..
                                },
                                grouping: TriggerGrouping::EachOccurrence,
                            } if *subject == ObjectFilter::self_object()
                        )
                    {
                        return Err(DefinitionError::InvalidInstruction);
                    }
                    None
                }
                Instruction::TransformSourceAndAttachToTarget => {
                    if self.target
                        != BodyTarget::FilteredPermanent(
                            ObjectFilter::creature().of_related_player(),
                        )
                        || !matches!(
                            context,
                            BodyContext::TriggerBound {
                                condition,
                                grouping: TriggerGrouping::EachOccurrence,
                            } if *condition == TriggerSpec::self_deals_combat_damage_to_player()
                        )
                    {
                        return Err(DefinitionError::InvalidInstruction);
                    }
                    Some(OperandFamily::Permanent)
                }
                Instruction::RevealTopAndRemoveAttackerIfForest => {
                    if self.target != BodyTarget::None
                        || !matches!(
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
                        )
                    {
                        return Err(DefinitionError::InvalidInstruction);
                    }
                    None
                }
                Instruction::ControlledSubtypeDamagesAttacker(kind) => {
                    if vocabulary.label(*kind).is_none()
                        || !matches!(
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
                        )
                    {
                        return Err(DefinitionError::InvalidInstruction);
                    }
                    None
                }
                Instruction::GiveControlOfAbilitySource => {
                    if !matches!(
                        self.target,
                        BodyTarget::SinglePlayer | BodyTarget::SingleOpponent
                    ) || !matches!(
                        context,
                        BodyContext::TriggerBound { .. } | BodyContext::Activated { .. }
                    ) {
                        return Err(DefinitionError::InvalidInstruction);
                    }
                    Some(OperandFamily::Player)
                }
                Instruction::ChangeControlledPlaneswalkers { parts, .. }
                | Instruction::ChangeAbilitySource { parts, .. } => {
                    parts
                        .validate(vocabulary, &defined_slots)
                        .map_err(|_| DefinitionError::ContinuousEffect)?;
                    None
                }
                Instruction::CopyOtherControlledNontokensAsEntryHost => {
                    if self.target != BodyTarget::None
                        || !matches!(
                            context,
                            BodyContext::TriggerBound {
                                condition: TriggerSpec {
                                    event: TriggerEvent::Enters {
                                        attached_to_creature: true,
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
                Instruction::AttachBattlefieldAurasToSource
                | Instruction::EnterHandOrGraveyardAurasAttachedToSource => {
                    if self.target != BodyTarget::None
                        || !matches!(
                            context,
                            BodyContext::TriggerBound { .. } | BodyContext::Activated { .. }
                        )
                    {
                        return Err(DefinitionError::InvalidInstruction);
                    }
                    None
                }
                Instruction::UntapControlledLands
                | Instruction::UntapAndControlCreatures
                | Instruction::Proliferate
                | Instruction::AttachControlledToSource { .. }
                | Instruction::CopyDifferentlyNamedTokens
                | Instruction::DamagePlayersHalfLife => None,
                Instruction::DamageTargetControllersCreatures { amount, .. } => {
                    if !matches!(
                        self.target,
                        BodyTarget::SinglePlayer | BodyTarget::SinglePlayerOrPlaneswalker
                    ) {
                        return Err(DefinitionError::OperandFamily);
                    }
                    amount.validate_amount(&defined_slots, context, &facts, vocabulary)?;
                    Some(OperandFamily::DamageRecipient)
                }
                Instruction::OfferOpponentsDamage => {
                    if self.target != BodyTarget::None
                        || !matches!(context, BodyContext::TriggerBound {
                            condition,
                            grouping: TriggerGrouping::EachOccurrence,
                        } if *condition == TriggerSpec::self_enters())
                        || !matches!(
                            self.instructions.as_slice(),
                            [
                                Instruction::OfferOpponentsDamage,
                                Instruction::DamageGroup {
                                    group: DamageGroup::VolunteeringOpponent,
                                    ..
                                },
                                Instruction::SacrificeStateTriggerSource,
                            ]
                        )
                    {
                        return Err(DefinitionError::InvalidInstruction);
                    }
                    None
                }
                Instruction::DamageGroup { amount, group } => {
                    if *group == DamageGroup::VolunteeringOpponent
                        && self.instructions.first() != Some(&Instruction::OfferOpponentsDamage)
                    {
                        return Err(DefinitionError::InvalidInstruction);
                    }
                    if matches!(
                        group,
                        DamageGroup::RelatedPlayer | DamageGroup::RelatedPlayerOrTheirPlaneswalker
                    ) {
                        ResolvingPlayer::UniqueRelatedPlayer.validate(
                            context,
                            self.target,
                            false,
                        )?;
                    }
                    if *group == DamageGroup::DestroyedTargetController
                        && index
                            .checked_sub(1)
                            .and_then(|previous| self.instructions.get(previous))
                            != Some(&Instruction::DestroyTarget)
                    {
                        return Err(DefinitionError::InvalidInstruction);
                    }
                    if *group == DamageGroup::CapturedAttackedPlayerOrPlaneswalker
                        && !matches!(context, BodyContext::TriggerBound {
                            condition: TriggerSpec {
                                event: TriggerEvent::Attacks {
                                    subject,
                                    mode: AttackTriggerMode::Individual { capture_defender: true },
                                },
                                ..
                            },
                            grouping: TriggerGrouping::EachOccurrence,
                        } if *subject == ObjectFilter::creature().controlled())
                    {
                        return Err(DefinitionError::InvalidInstruction);
                    }
                    if *group == DamageGroup::ControllerAndTargetCreature {
                        if self.target != BodyTarget::SingleCreature {
                            return Err(DefinitionError::OperandFamily);
                        }
                        if !amount.is_positive() {
                            return Err(DefinitionError::InvalidInstruction);
                        }
                    }
                    if matches!(group, DamageGroup::CreaturesExcept(kind) | DamageGroup::CreaturesOfSubtype(kind) if vocabulary.label(*kind).is_none())
                        || !amount.is_positive()
                    {
                        return Err(DefinitionError::InvalidInstruction);
                    }
                    (*group == DamageGroup::ControllerAndTargetCreature)
                        .then_some(OperandFamily::Permanent)
                }
                Instruction::PutHandPermanentsOntoBattlefield {
                    player,
                    types,
                    maximum,
                    ..
                } => {
                    if *maximum == 0
                        || types.is_empty()
                        || types.iter().any(|kind| !kind.is_permanent())
                        || !matches!(
                            player,
                            ResolvingPlayer::Controller | ResolvingPlayer::UniqueRelatedPlayer
                        )
                        || self.target != BodyTarget::None
                    {
                        return Err(DefinitionError::InvalidInstruction);
                    }
                    player.validate(context, self.target, facts.definitely_exiled_target)?
                }
                Instruction::OrderArtifactCreatureLandSacrifices => {
                    if self.target != BodyTarget::None
                        || self.instructions.len() != 1
                        || !matches!(context, BodyContext::Unbound)
                    {
                        return Err(DefinitionError::InvalidInstruction);
                    }
                    None
                }
                Instruction::ChooseTypeThenDestroyUnchosen => {
                    if self.target != BodyTarget::None
                        || self.instructions.len() != 1
                        || vocabulary.is_empty()
                    {
                        return Err(DefinitionError::InvalidInstruction);
                    }
                    None
                }
                Instruction::ChooseCreatureTypeForPlayer { .. } => {
                    return Err(DefinitionError::InvalidInstruction);
                }
                Instruction::ChooseCreatureType { into, excluding } => {
                    if excluding
                        .iter()
                        .any(|value| vocabulary.label(*value).is_none())
                    {
                        return Err(DefinitionError::ForeignCreatureType);
                    }
                    if excluding.len() == vocabulary.len() {
                        return Err(DefinitionError::EmptyCreatureTypeDomain);
                    }
                    if !defined_slots.insert(*into) {
                        return Err(DefinitionError::DuplicateCreatureTypeSlot);
                    }
                    None
                }
                Instruction::ManifestTop { player, amount }
                | Instruction::Scry { player, amount }
                | Instruction::Surveil { player, amount }
                | Instruction::Mill { player, amount } => {
                    if matches!(instruction, Instruction::Mill { .. }) {
                        facts.definitely_milled_top = true;
                    }
                    let family =
                        player.validate(context, self.target, facts.definitely_exiled_target)?;
                    amount.validate_amount(&defined_slots, context, &facts, vocabulary)?;
                    family
                }
                Instruction::WatchSmallCreatureCombatDraw(_)
                    if self.target == BodyTarget::None
                        && matches!(context, BodyContext::Activated { .. }) =>
                {
                    None
                }
                Instruction::WatchTargetCombatDamageDraw
                    if self.target
                        == BodyTarget::FilteredPermanent(ObjectFilter::creature().controlled()) =>
                {
                    Some(OperandFamily::Permanent)
                }
                Instruction::WatchSmallCreatureCombatDraw(_)
                | Instruction::WatchTargetCombatDamageDraw => {
                    return Err(DefinitionError::InvalidInstruction);
                }
                Instruction::ChooseOwnCommanderToCast
                    if self.target == BodyTarget::None
                        && matches!(context,BodyContext::TriggerBound {condition,..} if *condition == TriggerSpec::self_deals_combat_damage_to_player()) =>
                {
                    None
                }
                Instruction::SearchEachLibraryForFreeCast
                    if self.instructions.len() == 1 && self.target == BodyTarget::None =>
                {
                    None
                }
                Instruction::MayTurnControlledCreatureFaceUp
                | Instruction::ExploreSource
                | Instruction::ExileNonlandsToHandAndDamage { .. }
                | Instruction::RevealTopHandLoseManaValue { .. }
                | Instruction::RevealSharedTypeCreatureCastOrBottom
                | Instruction::RevealOddCastOrDraw
                | Instruction::RevealUntilLandKind
                | Instruction::RevealOpponentChoosesCreature
                | Instruction::ShuffleAbilitySourceIntoLibrary
                | Instruction::RevealToNameAfterSourceShuffle(_)
                | Instruction::RevealHandAndBottomPiles
                | Instruction::RevealDividePiles
                | Instruction::DrawToChosenOpponentHandSize
                | Instruction::LookTopPlaneswalkers
                | Instruction::RevealTopPermanentOrHand
                    if self.target == BodyTarget::None =>
                {
                    None
                }
                Instruction::MayTurnControlledCreatureFaceUp
                | Instruction::ExploreSource
                | Instruction::ExileNonlandsToHandAndDamage { .. }
                | Instruction::RevealTopHandLoseManaValue { .. }
                | Instruction::RevealSharedTypeCreatureCastOrBottom
                | Instruction::RevealOddCastOrDraw
                | Instruction::RevealUntilLandKind
                | Instruction::RevealOpponentChoosesCreature
                | Instruction::ChooseOwnCommanderToCast
                | Instruction::SearchEachLibraryForFreeCast
                | Instruction::RevealHandAndBottomPiles
                | Instruction::RevealDividePiles
                | Instruction::DrawToChosenOpponentHandSize
                | Instruction::LookTopPlaneswalkers
                | Instruction::RevealTopPermanentOrHand => {
                    return Err(DefinitionError::InvalidInstruction);
                }
                Instruction::ReturnCardsDestroyedThisWay => {
                    if !index
                        .checked_sub(1)
                        .and_then(|previous| self.instructions.get(previous))
                        .is_some_and(|op| {
                            matches!(
                                op,
                                Instruction::AffectPermanentTargets {
                                    action: PermanentTargetAction::Destroy,
                                    ..
                                }
                            )
                        })
                    {
                        return Err(DefinitionError::InvalidInstruction);
                    }
                    None
                }
                Instruction::DivideTargetPermanentsAndSacrificePile => {
                    if self.target != BodyTarget::SinglePlayer {
                        return Err(DefinitionError::OperandFamily);
                    }
                    Some(OperandFamily::Player)
                }
                Instruction::LookTopLandBattlefieldTapped if self.target == BodyTarget::None => {
                    None
                }
                Instruction::LookTopLandBattlefieldTapped => {
                    return Err(DefinitionError::InvalidInstruction);
                }
                Instruction::LookTopReveal { count, .. } => {
                    if *count == 0 || self.target != BodyTarget::None {
                        return Err(DefinitionError::InvalidInstruction);
                    }
                    None
                }
                Instruction::LookTopTakeOne { count } => {
                    if *count == 0 || *count > 2 || self.target != BodyTarget::None {
                        return Err(DefinitionError::InvalidInstruction);
                    }
                    None
                }
                Instruction::FixedTargetClause(body) => {
                    if self.target != BodyTarget::None
                        || body.target == BodyTarget::None
                        || body.instructions.len() != 1
                        || !body.instructions[0].uses_mode_target()
                    {
                        return Err(DefinitionError::InvalidInstruction);
                    }
                    body.validate_in(vocabulary, context)?;
                    None
                }
                Instruction::DistributePlusOneCounters { total } => {
                    if *total == 0
                        || self.target != BodyTarget::SingleCreature
                        || self.instructions.len() != 1
                        || !matches!(context, BodyContext::Unbound)
                    {
                        return Err(DefinitionError::InvalidInstruction);
                    }
                    Some(OperandFamily::Permanent)
                }
                Instruction::DivideThreeDamageAmongTargets => {
                    if self.target != BodyTarget::SingleAnyTarget
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
                Instruction::DamageOrderedTargets { first, others } => {
                    let dependent = self.target == BodyTarget::OpponentOrPlaneswalkerThenCreature
                        && matches!(
                            context,
                            BodyContext::TriggerBound {
                                grouping: TriggerGrouping::EachOccurrence,
                                ..
                            }
                        );
                    if !(dependent || self.target == BodyTarget::SingleAnyTarget)
                        || self.instructions.len() != 1
                        || !first.is_positive()
                        || !others.is_positive()
                    {
                        return Err(DefinitionError::InvalidInstruction);
                    }
                    Some(OperandFamily::DamageRecipient)
                }
                Instruction::ReturnChosenXGraveyardCreatures
                    if self.target
                        == BodyTarget::FilteredOwnGraveyardCard(ObjectFilter::creature())
                        && self.instructions.len() == 1 =>
                {
                    Some(OperandFamily::GraveyardCard)
                }
                Instruction::ReturnChosenXGraveyardCreatures => {
                    return Err(DefinitionError::InvalidInstruction);
                }
                Instruction::ReturnGraveyardTargetsTapped(count) => {
                    if *count == 0
                        || self.target
                            != BodyTarget::FilteredOwnGraveyardCard(ObjectFilter::creature())
                        || !matches!(
                            self.instructions.as_slice(),
                            [
                                Instruction::SacrificeToContinue { .. },
                                Instruction::ReturnGraveyardTargetsTapped(_)
                            ]
                        )
                    {
                        return Err(DefinitionError::InvalidInstruction);
                    }
                    Some(OperandFamily::GraveyardCard)
                }
                Instruction::ReturnGraveyardCohort(returning) => {
                    if self.target != BodyTarget::None
                        || matches!(returning.filter, GraveyardReturnFilter::CardType(kind) if !kind.is_permanent())
                        || matches!(returning.filter, GraveyardReturnFilter::CreatureSubtype(kind) if vocabulary.label(kind).is_none())
                        || returning
                            .additions
                            .creature_subtype
                            .is_some_and(|kind| vocabulary.label(kind).is_none())
                    {
                        return Err(DefinitionError::InvalidInstruction);
                    }
                    None
                }
                Instruction::ReturnCapturedDyingCreatureWithTypeAndColor { subtype, .. } => {
                    if self.target != BodyTarget::None
                        || vocabulary.label(*subtype).is_none()
                        || !matches!(
                            context,
                            BodyContext::TriggerBound {
                                condition: TriggerSpec {
                                    event: TriggerEvent::Dies {
                                        mode: DiesTriggerMode::DamagedBySource,
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
                Instruction::ReturnCapturedDyingCreature => {
                    if self.target != BodyTarget::None
                        || !matches!(
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
                        )
                    {
                        return Err(DefinitionError::InvalidInstruction);
                    }
                    None
                }
                Instruction::ReturnGraveyardCreatureWithSubtypeCounters { subtype, amount } => {
                    if self.target != BodyTarget::FilteredOwnGraveyardCard(ObjectFilter::creature())
                        || vocabulary.label(*subtype).is_none()
                        || *amount == 0
                    {
                        return Err(DefinitionError::InvalidInstruction);
                    }
                    Some(OperandFamily::GraveyardCard)
                }
                Instruction::CreatureDealsPowerDamageToOther { .. } => {
                    let [
                        Instruction::FixedTargetClause(dealer),
                        Instruction::FixedTargetClause(recipient),
                        Instruction::CreatureDealsPowerDamageToOther { .. },
                    ] = self.instructions.as_slice()
                    else {
                        return Err(DefinitionError::InvalidInstruction);
                    };
                    if self.target != BodyTarget::None
                        || dealer.target
                            != BodyTarget::FilteredPermanent(ObjectFilter::creature().controlled())
                        || recipient.target
                            != BodyTarget::FilteredPermanent(ObjectFilter::creature().opposing())
                        || dealer.instructions != [Instruction::RememberPermanentTarget]
                        || recipient.instructions != [Instruction::RememberPermanentTarget]
                    {
                        return Err(DefinitionError::InvalidInstruction);
                    }
                    None
                }
                Instruction::CopyOtherTargetAsArtifact => {
                    let [
                        Instruction::FixedTargetClause(receiver),
                        Instruction::FixedTargetClause(donor),
                        Instruction::CopyOtherTargetAsArtifact,
                    ] = self.instructions.as_slice()
                    else {
                        return Err(DefinitionError::InvalidInstruction);
                    };
                    if self.target != BodyTarget::None
                        || receiver.target
                            != BodyTarget::FilteredPermanent(ObjectFilter::artifact().controlled())
                        || donor.target
                            != BodyTarget::FilteredPermanent(
                                ObjectFilter::artifact_or_creature().controlled(),
                            )
                        || receiver.instructions != [Instruction::RememberPermanentTarget]
                        || donor.instructions != [Instruction::RememberPermanentTarget]
                    {
                        return Err(DefinitionError::InvalidInstruction);
                    }
                    None
                }
                Instruction::ExileUntilOtherTargetLeaves => {
                    if self.target != BodyTarget::None || self.instructions.len() != 3 || !matches!(self.instructions.last(), Some(Instruction::ExileUntilOtherTargetLeaves)) || !self.instructions[..2].iter().all(|op| matches!(op, Instruction::FixedTargetClause(body) if body.instructions == [Instruction::ExileTarget])) { return Err(DefinitionError::InvalidInstruction); }
                    None
                }
                Instruction::ReturnFixedTargets | Instruction::FightFixedTargets => {
                    let [
                        Instruction::FixedTargetClause(first),
                        Instruction::FixedTargetClause(second),
                        _,
                    ] = self.instructions.as_slice()
                    else {
                        return Err(DefinitionError::InvalidInstruction);
                    };
                    if self.target != BodyTarget::None
                        || if matches!(instruction, Instruction::FightFixedTargets) {
                            !first.target.guarantees_creature()
                                || !second.target.guarantees_creature()
                        } else {
                            first.target
                                != BodyTarget::FilteredPermanent(
                                    ObjectFilter::creature().controlled(),
                                )
                                || second.target
                                    != BodyTarget::FilteredPermanent(
                                        ObjectFilter::creature().not_controlled(),
                                    )
                        }
                        || first.instructions != [Instruction::RememberPermanentTarget]
                        || second.instructions != [Instruction::RememberPermanentTarget]
                        || index != 2
                    {
                        return Err(DefinitionError::InvalidInstruction);
                    }
                    None
                }
                Instruction::DestroyTargetAndAttachedEquipment => {
                    if !self.target.guarantees_creature() {
                        return Err(DefinitionError::OperandFamily);
                    }
                    Some(OperandFamily::Permanent)
                }
                Instruction::DestroyFixedTargets => {
                    if self.target != BodyTarget::None || !matches!(self.instructions.last(), Some(Instruction::DestroyFixedTargets)) || self.instructions.len() < 2 || !self.instructions[..self.instructions.len()-1].iter().all(|op| matches!(op, Instruction::FixedTargetClause(body) if body.instructions == [Instruction::DestroyTarget])) { return Err(DefinitionError::InvalidInstruction); }
                    None
                }
                Instruction::ExileResolvingSpell => {
                    if self.target != BodyTarget::None {
                        return Err(DefinitionError::InvalidInstruction);
                    }
                    None
                }
                Instruction::LookOpponentExileForCast { count }
                    if *count > 0 && self.target == BodyTarget::SingleOpponent =>
                {
                    Some(OperandFamily::Player)
                }
                Instruction::LookOpponentExileForCast { .. } => {
                    return Err(DefinitionError::InvalidInstruction);
                }
                Instruction::ExileFromTargetLibrary => Some(OperandFamily::Player),
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
                                != (Instruction::OptionalEffect {
                                    chooser: ResolvingPlayer::DestructionTargetController,
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
                        SearchCardMatcher::CreatureSubtype(kind)
                            if vocabulary.label(*kind).is_none() =>
                        {
                            return Err(DefinitionError::InvalidInstruction);
                        }
                        _ => {}
                    }
                    if matches!(
                        &filter.mana_value,
                        Some(SearchManaValue::Exact(value)) if value < &0.into()
                    ) {
                        return Err(DefinitionError::InvalidInstruction);
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
                        if self.target != BodyTarget::SingleOpponent {
                            return Err(DefinitionError::OperandFamily);
                        }
                        Some(OperandFamily::Player)
                    } else {
                        None
                    }
                }
                Instruction::OfferVariablePayment { .. }
                | Instruction::DrawEachPlayerForPayments
                | Instruction::CreateReflexiveGraveyardReturn => {
                    if self.target != BodyTarget::None
                        || !matches!(
                            self.instructions.as_slice(),
                            [
                                Instruction::OfferVariablePayment { each_player: true },
                                Instruction::DrawEachPlayerForPayments
                            ]
                        ) && !(matches!(
                            self.instructions.as_slice(),
                            [
                                Instruction::OfferVariablePayment { each_player: false },
                                Instruction::CreateReflexiveGraveyardReturn
                            ]
                        ) && matches!(context,BodyContext::TriggerBound {condition,..} if *condition == TriggerSpec::self_attacks()))
                    {
                        return Err(DefinitionError::InvalidInstruction);
                    }
                    None
                }
                Instruction::ReturnGraveyardCreatureWithCorpse(_) => {
                    return Err(DefinitionError::InvalidInstruction);
                }
                Instruction::ReturnRandomLinkedExile
                    if self.target == BodyTarget::None
                        && matches!(context,BodyContext::TriggerBound {condition, grouping:TriggerGrouping::EachOccurrence} if *condition == TriggerSpec::self_dies() || *condition == TriggerSpec::self_attacks()) =>
                {
                    None
                }
                Instruction::ReturnRandomLinkedExile => {
                    return Err(DefinitionError::InvalidInstruction);
                }
                Instruction::ReturnLinkedSearchCards
                    if self.target == BodyTarget::None
                        && matches!(context,BodyContext::TriggerBound {condition,grouping:TriggerGrouping::EachOccurrence} if *condition == TriggerSpec::self_dies()) =>
                {
                    None
                }
                Instruction::ReturnLinkedSearchCards => {
                    return Err(DefinitionError::InvalidInstruction);
                }
                Instruction::DrawControllerAndTarget { amount }
                | Instruction::DrawTargetPlayer { amount } => {
                    if matches!(instruction, Instruction::DrawControllerAndTarget { .. })
                        && self.target != BodyTarget::SingleOpponent
                    {
                        return Err(DefinitionError::OperandFamily);
                    }
                    amount.validate_amount(&defined_slots, context, &facts, vocabulary)?;
                    Some(OperandFamily::Player)
                }
                Instruction::CreateReflexiveDiscardDamage => {
                    if self.target != BodyTarget::None {
                        return Err(DefinitionError::InvalidInstruction);
                    }
                    None
                }
                Instruction::CreateNonlegendaryLoyaltyCopy => {
                    if self.target != BodyTarget::None
                        || !matches!(
                            context,
                            BodyContext::TriggerBound {
                                condition: TriggerSpec {
                                    event: TriggerEvent::Enters {
                                        cast_origin: Some(CastEntryKind::Kicked),
                                        ..
                                    },
                                    ..
                                },
                                grouping: TriggerGrouping::EachOccurrence
                            }
                        )
                    {
                        return Err(DefinitionError::InvalidInstruction);
                    }
                    None
                }
                Instruction::ReselectAttackDefender
                    if self.target
                        == BodyTarget::FilteredPermanent(ObjectFilter::creature().attacking()) =>
                {
                    Some(OperandFamily::Permanent)
                }
                Instruction::ReselectAttackDefender => {
                    return Err(DefinitionError::InvalidInstruction);
                }
                Instruction::SetBlockDeclarations(_)
                    if self.target == BodyTarget::None
                        && matches!(context, BodyContext::TriggerBound { condition: TriggerSpec { event: TriggerEvent::Attacks { subject, mode: AttackTriggerMode::Individual { capture_defender: false } | AttackTriggerMode::WithAtLeastOthers(_) }, intervening_if: None, .. }, .. } if *subject == ObjectFilter::self_object()) =>
                {
                    None
                }
                Instruction::SetBlockDeclarations(_) => {
                    return Err(DefinitionError::InvalidInstruction);
                }
                Instruction::PhaseOtherPlaneswalkersNextEndStep
                    if matches!(context, BodyContext::Activated { .. })
                        && self.target
                            == BodyTarget::FilteredPermanent(
                                ObjectFilter::planeswalker()
                                    .controlled()
                                    .other_than_trigger_source(),
                            ) =>
                {
                    Some(OperandFamily::Permanent)
                }
                Instruction::ReturnOneNonlandPerOpponent
                    if self.target
                        == BodyTarget::FilteredPermanent(ObjectFilter::nonland().opposing()) =>
                {
                    Some(OperandFamily::Permanent)
                }
                Instruction::ChooseMeldPartner(_)
                | Instruction::ExileMeldPair
                | Instruction::MeldExiledPair(_) => {
                    if !matches!(
                        self.instructions.as_slice(),
                        [
                            Instruction::ChooseMeldPartner(_),
                            Instruction::ExileMeldPair,
                            Instruction::MeldExiledPair(_)
                        ]
                    ) || !matches!(
                        context,
                        BodyContext::TriggerBound {
                            condition: TriggerSpec {
                                intervening_if: Some(TriggerCondition::ControlsMeldPair),
                                ..
                            },
                            grouping: TriggerGrouping::EachOccurrence
                        }
                    ) || self.target != BodyTarget::None
                    {
                        return Err(DefinitionError::InvalidInstruction);
                    }
                    if let Instruction::MeldExiledPair(back) = instruction {
                        if back.permanent().is_none() || back.printed_mana_cost().is_some() {
                            return Err(DefinitionError::InvalidInstruction);
                        }
                        back.validate(vocabulary)?;
                    }
                    None
                }
                Instruction::PairWithCreature { entrant_only } => {
                    if self.target != BodyTarget::None
                        || !matches!(context, BodyContext::TriggerBound {
                        condition: TriggerSpec { event: TriggerEvent::Keyword(TriggerKeywordEvent::Soulbond { entrant_only: only }), .. },
                        grouping: TriggerGrouping::EachOccurrence,
                    } if *only == *entrant_only)
                    {
                        return Err(DefinitionError::InvalidInstruction);
                    }
                    None
                }
                Instruction::PhaseOut(PhasingSubject::Target) => Some(OperandFamily::Permanent),
                Instruction::PhaseOut(PhasingSubject::Source) => {
                    if self.target != BodyTarget::None
                        || !matches!(
                            context,
                            BodyContext::TriggerBound { .. } | BodyContext::Activated { .. }
                        )
                    {
                        return Err(DefinitionError::InvalidInstruction);
                    }
                    None
                }
                Instruction::PhaseOut(PhasingSubject::ControlledLands) => {
                    if self.target != BodyTarget::None {
                        return Err(DefinitionError::OperandFamily);
                    }
                    None
                }
                Instruction::ReturnOneNonlandPerOpponent
                | Instruction::PhaseOtherPlaneswalkersNextEndStep
                | Instruction::PhaseOutObjects(_)
                | Instruction::WeakenDelayedAttacker(_) => {
                    return Err(DefinitionError::InvalidInstruction);
                }
                Instruction::WeakenOpposingAttackersUntilNextTurn
                | Instruction::DrawRevealRemoveLoyalty => {
                    if self.target != BodyTarget::None
                        || matches!(context, BodyContext::TriggerBound { .. })
                    {
                        return Err(DefinitionError::InvalidInstruction);
                    }
                    None
                }
                Instruction::RemoveLoyaltyExileTopForPlay => {
                    if self.target != BodyTarget::None {
                        return Err(DefinitionError::InvalidInstruction);
                    }
                    None
                }
                Instruction::AbandonSourceScheme => {
                    if self.target != BodyTarget::None {
                        return Err(DefinitionError::InvalidInstruction);
                    }
                    None
                }
                Instruction::TabletopHonorAction { prompt } => {
                    if prompt.trim().is_empty() {
                        return Err(DefinitionError::InvalidInstruction);
                    }
                    None
                }
                Instruction::AddMana(mana_yield) => {
                    match mana_yield {
                        EffectManaYield::RestrictedChoice {
                            colors,
                            restriction,
                        } => {
                            if self.target != BodyTarget::None
                                || colors.is_empty()
                                || colors.contains(&crate::mana::ManaType::Colourless)
                                || *restriction != crate::mana::ManaRestriction::NoncreatureSpell
                            {
                                return Err(DefinitionError::InvalidInstruction);
                            }
                            None
                        }
                        EffectManaYield::CombatDamageUntilCleanup(_) => {
                            ValueExpr::event(EventValueKind::CombatDamageToPlayer)
                                .validate_amount(&defined_slots, context, &facts, vocabulary)?;
                            if self.target != BodyTarget::None {
                                return Err(DefinitionError::InvalidInstruction);
                            }
                            None
                        }
                        EffectManaYield::Split { first, second, .. } if first == second => {
                            return Err(DefinitionError::InvalidInstruction);
                        }
                        EffectManaYield::Counted { amount, .. }
                        | EffectManaYield::Split { amount, .. } => {
                            if self.target != BodyTarget::None {
                                return Err(DefinitionError::InvalidInstruction);
                            }
                            amount.validate_amount(&defined_slots, context, &facts, vocabulary)?;
                            None
                        }
                        EffectManaYield::Fixed(produced) => {
                            if produced.is_empty()
                                || produced.values().any(|amount| !amount.is_positive())
                            {
                                return Err(DefinitionError::InvalidInstruction);
                            }
                            None
                        }
                    }
                }
                Instruction::DamageDamagedPlayersCreatures => {
                    ValueExpr::event(EventValueKind::CombatDamageToPlayer).validate_amount(
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
                Instruction::DamageRelatedPlayerAmount(amount) => {
                    if self.target != BodyTarget::None {
                        return Err(DefinitionError::OperandFamily);
                    }
                    ResolvingPlayer::UniqueRelatedPlayer.validate(context, self.target, false)?;
                    amount.validate_amount(&defined_slots, context, &facts, vocabulary)?;
                    None
                }
                Instruction::DamageOpponentsAmount(amount) => {
                    if self.target != BodyTarget::None {
                        return Err(DefinitionError::InvalidInstruction);
                    }
                    amount.validate_amount(&defined_slots, context, &facts, vocabulary)?;
                    None
                }
                Instruction::ExileTopRepeatOnPermanentEntry => {
                    if self.target != BodyTarget::None || self.instructions.len() != 1 {
                        return Err(DefinitionError::InvalidInstruction);
                    }
                    None
                }
                Instruction::TargetDamagesItsControllersOtherCreatures
                | Instruction::OtherCreaturesReturnPowerDamage => {
                    if self.target
                        != BodyTarget::FilteredPermanent(ObjectFilter::creature().opposing())
                        || self.instructions
                            != [
                                Instruction::TargetDamagesItsControllersOtherCreatures,
                                Instruction::OtherCreaturesReturnPowerDamage,
                            ]
                    {
                        return Err(DefinitionError::InvalidInstruction);
                    }
                    Some(OperandFamily::Permanent)
                }
                Instruction::ChooseAndReturnOwnGraveyardCardsByType
                    if self.target == BodyTarget::None =>
                {
                    None
                }
                Instruction::ReturnOwnGraveyardCardsOfTypeToHand(kind)
                    if self.target == BodyTarget::None && kind.is_permanent() =>
                {
                    None
                }
                Instruction::ChooseAndReturnOwnGraveyardCardsByType
                | Instruction::ReturnOwnGraveyardCardsOfTypeToHand(_) => {
                    return Err(DefinitionError::InvalidInstruction);
                }
                Instruction::ReflectAttachedCreatureDamage
                    if self.target == BodyTarget::None
                        && matches!(context, BodyContext::TriggerBound {
                        condition: TriggerSpec {
                            event: TriggerEvent::DealtDamage { subject, linked_player: false },
                            ..
                        },
                        grouping: TriggerGrouping::OneOrMoreTogether,
                    } if *subject == ObjectFilter::any().attached_to_self()) =>
                {
                    None
                }
                Instruction::ReflectAttachedCreatureDamage => {
                    return Err(DefinitionError::InvalidInstruction);
                }
                Instruction::DestroyAttachedEnchantmentSubtype(_)
                    if self.target == BodyTarget::None =>
                {
                    None
                }
                Instruction::DestroyAttachedEnchantmentSubtype(_) => {
                    return Err(DefinitionError::OperandFamily);
                }
                Instruction::ExileTargetWithLink(group) => {
                    let admitted = match (group, self.target, context) {
                        (
                            LinkedExileGroup::FiendHunter,
                            BodyTarget::FilteredPermanent(filter),
                            BodyContext::TriggerBound {
                                condition,
                                grouping: TriggerGrouping::EachOccurrence,
                            },
                        ) if *condition == TriggerSpec::self_enters()
                            && filter == ObjectFilter::creature().other_than_trigger_source() =>
                        {
                            true
                        }
                        (
                            LinkedExileGroup::Helvault,
                            BodyTarget::FilteredPermanent(filter),
                            BodyContext::Activated { .. },
                        ) if filter == ObjectFilter::creature().controlled()
                            || filter == ObjectFilter::creature().not_controlled() =>
                        {
                            true
                        }
                        _ => false,
                    };
                    if !admitted {
                        return Err(DefinitionError::InvalidInstruction);
                    }
                    Some(OperandFamily::Permanent)
                }
                Instruction::ReturnCardsExiledWithLink(group) => {
                    let expected = match group {
                        LinkedExileGroup::FiendHunter => {
                            TriggerSpec::leaves_battlefield(ObjectFilter::self_object())
                        }
                        LinkedExileGroup::Helvault => TriggerSpec::new(TriggerEvent::Dies {
                            subject: ObjectFilter::self_object(),
                            mode: DiesTriggerMode::Permanent,
                        }),
                    };
                    if self.target != BodyTarget::None
                        || !matches!(context, BodyContext::TriggerBound {
                        condition, grouping: TriggerGrouping::EachOccurrence,
                    } if *condition == expected)
                    {
                        return Err(DefinitionError::InvalidInstruction);
                    }
                    None
                }
                Instruction::ExileRelatedPermanents(group) => match (group, self.target) {
                    (RelatedExileGroup::CreaturesSharingTargetName, BodyTarget::SingleCreature) => {
                        Some(OperandFamily::Permanent)
                    }
                    (
                        RelatedExileGroup::TargetPlayerNonlandsUntilNextEndStep,
                        BodyTarget::SinglePlayer,
                    ) => Some(OperandFamily::Player),
                    _ => return Err(DefinitionError::InvalidInstruction),
                },
                Instruction::ExileTargetUntilNonlandForFreeCast => {
                    if self.target != BodyTarget::SingleOpponent {
                        return Err(DefinitionError::InvalidInstruction);
                    }
                    Some(OperandFamily::Player)
                }
                Instruction::ExileRelatedTopForCastThisTurn
                | Instruction::ExileRelatedTopForPlay
                    if self.target == BodyTarget::None =>
                {
                    ResolvingPlayer::UniqueRelatedPlayer.validate(context, self.target, false)?
                }
                Instruction::ExileRelatedTopForCastThisTurn
                | Instruction::ExileRelatedTopForPlay => {
                    return Err(DefinitionError::InvalidInstruction);
                }
                Instruction::PutLandFromExiledCohort => {
                    if self.target != BodyTarget::None
                        || index == 0
                        || !matches!(
                            self.instructions.get(index - 1),
                            Some(Instruction::ExileTopForEachSpellType(_))
                        )
                    {
                        return Err(DefinitionError::InvalidInstruction);
                    }
                    None
                }
                Instruction::ExileTopForEachSpellType(count)
                | Instruction::ExileTopFreeNoncreatureSpells(count)
                    if *count > 0 && self.target == BodyTarget::None =>
                {
                    None
                }
                Instruction::ExileTopForEachSpellType(_)
                | Instruction::ExileTopFreeNoncreatureSpells(_) => {
                    return Err(DefinitionError::InvalidInstruction);
                }
                Instruction::ExileTopForPlay {
                    until_next_from_source,
                } => {
                    if self.target != BodyTarget::None
                        || *until_next_from_source
                            && !matches!(context, BodyContext::TriggerBound { .. })
                    {
                        return Err(DefinitionError::InvalidInstruction);
                    }
                    None
                }
                Instruction::ScheduleCreatedTokenSacrifice => {
                    if self.target != BodyTarget::None
                        || index == 0
                        || !matches!(
                            self.instructions[index - 1],
                            Instruction::CreateTokens { .. }
                        )
                        || !matches!(context, BodyContext::Unbound)
                    {
                        return Err(DefinitionError::InvalidInstruction);
                    }
                    None
                }
                Instruction::ScheduleCreatedTokenExile(step) => {
                    let previous = index
                        .checked_sub(1)
                        .and_then(|prior| self.instructions.get(prior));
                    let valid_source =
                        matches!(previous, Some(Instruction::ExileForSubtypeToken(_)))
                            || (self.target == BodyTarget::None
                                && matches!(
                                    previous,
                                    Some(
                                        Instruction::CreateTokens { .. }
                                            | Instruction::CreateAttackingTokens { .. }
                                            | Instruction::Myriad
                                    )
                                ));
                    if !matches!(step, Step::End | Step::EndCombat)
                        || !valid_source
                        || !matches!(context, BodyContext::TriggerBound { .. })
                    {
                        return Err(DefinitionError::InvalidInstruction);
                    }
                    None
                }
                Instruction::ExileTopFaceDown | Instruction::RegisterTokenLinkedExile => {
                    if self.target != BodyTarget::None {
                        return Err(DefinitionError::InvalidInstruction);
                    }
                    None
                }
                Instruction::ReturnTokenLinkedExile => {
                    return Err(DefinitionError::InvalidInstruction);
                }
                Instruction::ImpulseCastOrDamage(amount) => {
                    if !amount.is_positive() || self.target != BodyTarget::None {
                        return Err(DefinitionError::InvalidInstruction);
                    }
                    None
                }
                Instruction::CreateEmblem(definition)
                | Instruction::CreateOpponentEmblems(definition) => {
                    match definition {
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
                    }
                    None
                }
                Instruction::DrawController { amount }
                    if amount.is_target_player_tapped_creatures() =>
                {
                    if self.target != BodyTarget::SinglePlayer {
                        return Err(DefinitionError::OperandFamily);
                    }
                    Some(OperandFamily::Player)
                }
                Instruction::DrawAndDamageOpponents { amount }
                | Instruction::DrawEachPlayer { amount }
                | Instruction::DrawController { amount } => {
                    amount.validate_amount(&defined_slots, context, &facts, vocabulary)?;
                    None
                }
                Instruction::DrawPlayersControllingName(_) => None,
                Instruction::ModifyAnyNumberOfPermanents { parts, .. } => {
                    if !matches!(
                        self.target,
                        BodyTarget::FilteredPermanent(_) | BodyTarget::SinglePermanent
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
                    Some(OperandFamily::Permanent)
                }
                Instruction::SacrificeCombatContributor
                | Instruction::RevealCreatureOfSacrificedType => {
                    if self.target != BodyTarget::None
                        || !matches!(
                            context,
                            BodyContext::TriggerBound {
                                condition: TriggerSpec {
                                    event: TriggerEvent::DealsDamage {
                                        source,
                                        recipient: DamageTriggerRecipient::AnyPlayer,
                                        combat_only: true,
                                    },
                                    ..
                                },
                                grouping: TriggerGrouping::OneOrMoreByPlayer,
                            } if *source == ObjectFilter::creature().controlled()
                        )
                        || !matches!(
                            self.instructions.as_slice(),
                            [
                                Instruction::OptionalSequence {
                                    chooser: ResolvingPlayer::Controller,
                                    instructions: 2
                                },
                                Instruction::SacrificeCombatContributor,
                                Instruction::RevealCreatureOfSacrificedType
                            ]
                        )
                    {
                        return Err(DefinitionError::InvalidInstruction);
                    }
                    None
                }
                Instruction::ShuffleOnePermanentPerPlayer
                | Instruction::ExileNonlandForFormerControllers => {
                    if self.target
                        != BodyTarget::FilteredPermanent(ObjectFilter::nonland().nonenchantment())
                        || self.instructions
                            != [
                                Instruction::ShuffleOnePermanentPerPlayer,
                                Instruction::ExileNonlandForFormerControllers,
                            ]
                    {
                        return Err(DefinitionError::InvalidInstruction);
                    }
                    (index == 0).then_some(OperandFamily::Permanent)
                }
                Instruction::ExileMilledCreatures { maximum } => {
                    if *maximum == 0 || self.target != BodyTarget::None || !facts.definitely_milled
                    {
                        return Err(DefinitionError::InvalidInstruction);
                    }
                    facts.definitely_exiled_milled = true;
                    None
                }
                Instruction::VoteForGraveyardCards { types } => {
                    if types.is_empty() || self.target != BodyTarget::None {
                        return Err(DefinitionError::InvalidInstruction);
                    }
                    None
                }
                Instruction::MayRevealLibraryTop => {
                    if self.target != BodyTarget::None {
                        return Err(DefinitionError::OperandFamily);
                    }
                    facts.definitely_checked_top = true;
                    None
                }
                Instruction::LookTopOneRestGraveyard { count }
                | Instruction::RevealTopHandRestGraveyard { count, .. }
                | Instruction::RevealTopCreaturesAndLandsRestBottom { count } => {
                    if *count == 0 || self.target != BodyTarget::None {
                        return Err(DefinitionError::InvalidInstruction);
                    }
                    None
                }
                Instruction::LookTopOneRestBottom { count } => {
                    if self.target != BodyTarget::None
                        || matches!(count, ValueExpr::Literal(value) if !value.is_positive())
                    {
                        return Err(DefinitionError::InvalidInstruction);
                    }
                    count.validate_amount(&defined_slots, context, &facts, vocabulary)?;
                    None
                }
                Instruction::LookTopCreatureSharingAttachedSubtype { count } => {
                    if self.target != BodyTarget::None
                        || *count == 0
                        || !matches!(context, BodyContext::TriggerBound { .. })
                    {
                        return Err(DefinitionError::InvalidInstruction);
                    }
                    None
                }
                Instruction::ReturnOddGraveyardPermanentIfPaid { minimum, .. } => {
                    if *minimum == 0
                        || index != 1
                        || !matches!(self.target, BodyTarget::FilteredPermanent(filter) if filter.is_opposing_even_mana_value())
                        || !matches!(
                            self.instructions.first(),
                            Some(Instruction::AffectPermanentTargets {
                                minimum: 0,
                                action: PermanentTargetAction::Exile,
                                ..
                            })
                        )
                    {
                        return Err(DefinitionError::InvalidInstruction);
                    }
                    None
                }
                Instruction::PutGraveyardCreatureAtMostX => {
                    if self.target != BodyTarget::None {
                        return Err(DefinitionError::InvalidInstruction);
                    }
                    ValueExpr::chosen_x().validate_amount(
                        &defined_slots,
                        context,
                        &facts,
                        vocabulary,
                    )?;
                    None
                }
                Instruction::RevealCreatureForEachTargetController
                | Instruction::ReturnExiledTargetCohort { .. } => {
                    let optional_return = index == 2
                        && matches!(
                            self.instructions.as_slice(),
                            [
                                Instruction::OptionalSequence {
                                    chooser: ResolvingPlayer::Controller,
                                    instructions: 2
                                },
                                Instruction::AffectPermanentTargets {
                                    action: PermanentTargetAction::Exile,
                                    ..
                                },
                                Instruction::ReturnExiledTargetCohort { .. },
                            ]
                        );
                    let returning_source = index == 1
                        && self.target == BodyTarget::None
                        && matches!(
                            self.instructions.as_slice(),
                            [
                                Instruction::ExileAbilitySource,
                                Instruction::ReturnExiledTargetCohort {
                                    next_end_step: false,
                                    ..
                                },
                            ]
                        );
                    if !optional_return
                        && !returning_source
                        && (index != 1
                            || !matches!(
                                self.instructions.as_slice(),
                                [
                                    Instruction::AffectPermanentTargets {
                                        action: PermanentTargetAction::Exile,
                                        ..
                                    },
                                    Instruction::RevealCreatureForEachTargetController
                                        | Instruction::ReturnExiledTargetCohort { .. }
                                        | Instruction::ReturnOddGraveyardPermanentIfPaid { .. },
                                    ..
                                ]
                            ))
                    {
                        return Err(DefinitionError::InvalidInstruction);
                    }
                    None
                }
                Instruction::AffectPermanentTargets {
                    minimum,
                    maximum,
                    action,
                } => {
                    if *action == PermanentTargetAction::PutPlusOneCounterOnOthers
                        && !matches!(context, BodyContext::TriggerBound { .. })
                    {
                        return Err(DefinitionError::InvalidInstruction);
                    }
                    let payment_then_freeze = matches!(
                        self.instructions.as_slice(),
                        [
                            Instruction::OfferManaPayment {
                                payer: ResolvingPlayer::Controller,
                                paid_destination: 1,
                                declined_destination: 2,
                                ..
                            },
                            Instruction::AffectPermanentTargets {
                                action: PermanentTargetAction::TapAndSkipNextUntap,
                                ..
                            },
                        ]
                    );
                    let untap_during_targeted_change = *minimum == 1
                        && *maximum == 1
                        && (matches!(
                            self.instructions.as_slice(),
                            [
                                Instruction::CreateContinuousEffect { .. },
                                Instruction::AffectPermanentTargets {
                                    action: PermanentTargetAction::Untap,
                                    ..
                                },
                                Instruction::CreateContinuousEffect { .. },
                            ]
                        ) || (index == 1
                            && matches!(
                                self.instructions.as_slice(),
                                [
                                    Instruction::CreateContinuousEffect { .. }
                                        | Instruction::PutPowerToughnessCounters { .. },
                                    Instruction::AffectPermanentTargets {
                                        action: PermanentTargetAction::Untap
                                            | PermanentTargetAction::Tap,
                                        ..
                                    },
                                ]
                            )));
                    let freeze_then_draw = index == 0
                        && matches!(
                            self.instructions.as_slice(),
                            [
                                Instruction::AffectPermanentTargets {
                                    minimum: 1,
                                    maximum: 1,
                                    action: PermanentTargetAction::TapAndSkipNextUntap
                                        | PermanentTargetAction::Tap
                                        | PermanentTargetAction::Untap,
                                },
                                Instruction::DrawController { .. }
                                    | Instruction::Scry {
                                        player: ResolvingPlayer::Controller,
                                        ..
                                    }
                                    | Instruction::Surveil {
                                        player: ResolvingPlayer::Controller,
                                        ..
                                    }
                                    | Instruction::AdjustControllerLife { .. }
                                    | Instruction::GainControllerLife { .. },
                            ]
                        );
                    let tap_before_other_effect = index == 0
                        && matches!(
                            self.instructions.as_slice(),
                            [
                                Instruction::AffectPermanentTargets {
                                    minimum: 1,
                                    maximum: 1,
                                    action: PermanentTargetAction::Tap
                                },
                                Instruction::BranchIfFalse { .. },
                                Instruction::ExileTarget,
                            ]
                        );
                    let optional_toggle = index == 1
                        && matches!(
                            self.instructions.as_slice(),
                            [
                                Instruction::OptionalEffect {
                                    chooser: ResolvingPlayer::Controller
                                },
                                Instruction::AffectPermanentTargets {
                                    minimum: 1,
                                    maximum: 1,
                                    action: PermanentTargetAction::ToggleTap
                                },
                            ]
                        );
                    if *action == PermanentTargetAction::ToggleTap && !optional_toggle {
                        return Err(DefinitionError::InvalidInstruction);
                    }
                    let tap_or_untap_sequence = (*minimum, *maximum) == (1, 1)
                        && match action {
                            PermanentTargetAction::Tap => {
                                index == 1
                                    && matches!(
                                        self.instructions.as_slice(),
                                        [
                                            Instruction::OptionalEffect { .. },
                                            Instruction::AffectPermanentTargets { .. }
                                        ]
                                    )
                            }
                            PermanentTargetAction::Untap => {
                                index == 0
                                    && matches!(
                                        self.instructions.as_slice(),
                                        [
                                            Instruction::AffectPermanentTargets { .. },
                                            Instruction::CreateContinuousEffect { .. }
                                        ]
                                    )
                            }
                            _ => false,
                        };
                    let optional_blink = index == 1
                        && matches!(
                            self.instructions.as_slice(),
                            [
                                Instruction::OptionalSequence {
                                    chooser: ResolvingPlayer::Controller,
                                    instructions: 2
                                },
                                Instruction::AffectPermanentTargets {
                                    minimum: 1,
                                    maximum: 1,
                                    action: PermanentTargetAction::Exile
                                },
                                Instruction::ReturnExiledTargetCohort {
                                    next_end_step: false,
                                    ..
                                },
                            ]
                        );
                    let destroy_then_return = *action == PermanentTargetAction::Destroy
                        && index == 0
                        && matches!(
                            self.instructions.as_slice(),
                            [_, Instruction::ReturnCardsDestroyedThisWay]
                        );
                    if minimum > maximum
                        || *maximum == 0
                        || self.instructions.len() != 1
                            && !payment_then_freeze
                            && !destroy_then_return
                            && !untap_during_targeted_change
                            && !tap_or_untap_sequence
                            && !freeze_then_draw
                            && !optional_blink
                            && !optional_toggle
                            && !tap_before_other_effect
                            && !matches!(
                                self.instructions.as_slice(),
                                [
                                    Instruction::AffectPermanentTargets {
                                        action: PermanentTargetAction::Exile,
                                        ..
                                    },
                                    Instruction::RevealCreatureForEachTargetController
                                        | Instruction::ReturnExiledTargetCohort { .. }
                                        | Instruction::ReturnOddGraveyardPermanentIfPaid { .. },
                                    ..
                                ]
                            )
                        || !matches!(
                            self.target,
                            BodyTarget::SingleCreature
                                | BodyTarget::SinglePermanent
                                | BodyTarget::FilteredPermanent(_)
                        )
                    {
                        return Err(DefinitionError::InvalidInstruction);
                    }
                    if let PermanentTargetAction::Modify { parts, .. }
                    | PermanentTargetAction::ModifyWithPartner { parts, .. } = action
                    {
                        let pair =
                            matches!(action, PermanentTargetAction::ModifyWithPartner { .. });
                        if pair && (*minimum, *maximum) != (1, 1) {
                            return Err(DefinitionError::InvalidInstruction);
                        }
                        if (*maximum > 1 || pair)
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
                        parts
                            .validate(vocabulary, &defined_slots)
                            .map_err(|_| DefinitionError::ContinuousEffect)?;
                    }
                    Some(OperandFamily::Permanent)
                }
                Instruction::PutLibraryCohortIntoGraveyard(
                    LibraryCohort::BottomCardOfTargetPlayer,
                ) => {
                    facts.moved_library_cards = true;
                    Some(OperandFamily::Player)
                }
                Instruction::PutLibraryCohortIntoGraveyard(
                    LibraryCohort::DefendingPlayerRevealsThroughLand,
                ) => {
                    if self.target != BodyTarget::None
                        || !matches!(context, BodyContext::TriggerBound {
                        condition: TriggerSpec {
                            event: TriggerEvent::Attacks {
                                subject,
                                mode: AttackTriggerMode::Individual { capture_defender: false },
                            },
                            ..
                        },
                        grouping: TriggerGrouping::EachOccurrence,
                    } if *subject == ObjectFilter::any().attached_to_self())
                    {
                        return Err(DefinitionError::InvalidInstruction);
                    }
                    facts.moved_library_cards = true;
                    None
                }
                Instruction::MillEachPlayer(_) => {
                    facts.definitely_milled = true;
                    None
                }
                Instruction::AddExperienceCounter => None,
                Instruction::GrantControllerFlashUntilCleanup => {
                    if self.target != BodyTarget::None {
                        return Err(DefinitionError::OperandFamily);
                    }
                    None
                }
                Instruction::ControlManaSourceThroughNextTurn => {
                    if self.target != BodyTarget::None
                        || !matches!(
                            context,
                            BodyContext::TriggerBound {
                                condition: TriggerSpec {
                                    event: TriggerEvent::PlayerAction {
                                        player: PlayerRelation::Opponent,
                                        action: PlayerTriggerAction::ArtifactTappedForMana,
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
                Instruction::PutNamedCounterOnSource { kind, amount } => {
                    if self.target != BodyTarget::None
                        || !matches!(
                            context,
                            BodyContext::TriggerBound { .. } | BodyContext::Activated { .. }
                        )
                        || !kind.is_named()
                        || *amount == 0
                    {
                        return Err(DefinitionError::InvalidInstruction);
                    }
                    None
                }
                Instruction::PayLifeOrSacrificeCreatures => {
                    if self.target != BodyTarget::None || !matches!(context, BodyContext::Unbound) {
                        return Err(DefinitionError::InvalidInstruction);
                    }
                    None
                }
                Instruction::ExileRelatedHandCardForPlay => {
                    ResolvingPlayer::UniqueRelatedPlayer.validate(context, self.target, false)?;
                    None
                }
                Instruction::ExileFromHandOrBattlefield(amount) => {
                    if self.target != BodyTarget::None {
                        return Err(DefinitionError::InvalidInstruction);
                    }
                    amount.validate_amount(&defined_slots, context, &facts, vocabulary)?;
                    None
                }
                Instruction::RequireOpponentsCreaturesBlockThisTurn => {
                    if self.target != BodyTarget::None {
                        return Err(DefinitionError::InvalidInstruction);
                    }
                    None
                }
                Instruction::RequireBlocksAgainstTargetThisTurn => {
                    if self.target != BodyTarget::SingleCreature {
                        return Err(DefinitionError::InvalidInstruction);
                    }
                    Some(OperandFamily::Permanent)
                }
                Instruction::SuppressGroupKeywords { group, keywords } => {
                    if keywords.is_empty() || !matches!(group.as_ref(), EffectSubject::Matching(_))
                    {
                        return Err(DefinitionError::InvalidInstruction);
                    }
                    group.validate_vocabulary(vocabulary)?;
                    None
                }
                Instruction::BindOrderedPermanents { minimum, maximum } => {
                    if index != 0
                        || *minimum == 0
                        || minimum > maximum
                        || !matches!(
                            self.target,
                            BodyTarget::SinglePermanent
                                | BodyTarget::SingleCreature
                                | BodyTarget::FilteredPermanent(_)
                        )
                        || !self
                            .instructions
                            .iter()
                            .any(|op| matches!(op, Instruction::ExileOrderedPermanent(_)))
                    {
                        return Err(DefinitionError::InvalidInstruction);
                    }
                    None
                }
                Instruction::ExileOrderedPermanent(position) => {
                    if !matches!(self.instructions.first(),Some(Instruction::BindOrderedPermanents {maximum,..}) if position<maximum)
                    {
                        return Err(DefinitionError::InvalidInstruction);
                    }
                    Some(OperandFamily::Permanent)
                }
                Instruction::PutTargetOwnedExiledLandsOntoBattlefield
                    if self.target == BodyTarget::SingleOpponent =>
                {
                    ResolvingPlayer::TargetPlayer.validate(context, self.target, false)?
                }
                Instruction::PutTargetOwnedExiledLandsOntoBattlefield => {
                    return Err(DefinitionError::InvalidInstruction);
                }
                Instruction::ExileLibraryTop { player, amount } => {
                    if *amount == 0
                        || !matches!(
                            player,
                            ResolvingPlayer::Controller
                                | ResolvingPlayer::TargetPlayer
                                | ResolvingPlayer::UniqueRelatedPlayer
                        )
                    {
                        return Err(DefinitionError::InvalidInstruction);
                    }
                    player.validate(context, self.target, false)?
                }
                Instruction::Bolster(0) => return Err(DefinitionError::InvalidInstruction),
                Instruction::Bolster(_) => None,
                Instruction::CreateArmyIfAbsent { army, subtype }
                | Instruction::Amass { army, subtype, .. } => {
                    if self.target != BodyTarget::None
                        || vocabulary.label(*army) != Some("Army")
                        || vocabulary.label(*subtype).is_none()
                        || matches!(instruction, Instruction::Amass { amount: 0, .. })
                    {
                        return Err(DefinitionError::InvalidInstruction);
                    }
                    None
                }
                Instruction::UntapUpTo { kind, .. } if !kind.is_permanent() => {
                    return Err(DefinitionError::InvalidInstruction);
                }
                Instruction::ChooseCreaturesToKeep | Instruction::DestroyOtherCreatures => {
                    if self.target != BodyTarget::None
                        || self.instructions
                            != [
                                Instruction::ChooseCreaturesToKeep,
                                Instruction::DestroyOtherCreatures,
                            ]
                    {
                        return Err(DefinitionError::InvalidInstruction);
                    }
                    None
                }
                Instruction::ChooseVowedCreatures | Instruction::SacrificeOtherCreatures => {
                    if self.target != BodyTarget::None
                        || self.instructions
                            != [
                                Instruction::ChooseVowedCreatures,
                                Instruction::SacrificeOtherCreatures,
                            ]
                    {
                        return Err(DefinitionError::InvalidInstruction);
                    }
                    None
                }
                Instruction::UntapUpTo { .. } | Instruction::SacrificeColoredPermanents => None,
                Instruction::ChooseOtherControlledCreature {
                    sacrifice,
                    or_exile_source,
                } => {
                    if *or_exile_source
                        && (!*sacrifice
                            || !matches!(
                                self.instructions.get(index + 1),
                                Some(Instruction::ExileAbilitySource)
                            ))
                    {
                        return Err(DefinitionError::InvalidInstruction);
                    }
                    if self.target != BodyTarget::None
                        || !matches!(context, BodyContext::TriggerBound { .. })
                    {
                        return Err(DefinitionError::InvalidInstruction);
                    }
                    None
                }
                Instruction::ExileChosenPermanents {
                    players,
                    amount,
                    filter,
                }
                | Instruction::ReturnControlledPermanents {
                    players,
                    amount,
                    filter,
                }
                | Instruction::Sacrifice {
                    players,
                    amount,
                    filter,
                } => {
                    let family =
                        players.validate(context, self.target, facts.definitely_exiled_target)?;
                    amount.validate_amount(&defined_slots, context, &facts, vocabulary)?;
                    filter.validate()?;
                    if matches!(instruction, Instruction::Sacrifice { .. }) {
                        facts.definitely_sacrificed = matches!(players, AffectedPlayers::One(_))
                            && matches!(amount, ValueExpr::Literal(value) if *value == 1.into());
                    }
                    // A generic selection can move the later copy donor. Until
                    // copiable LKI exists, preserve the existing conservative
                    // CFG boundary. Exact zero adds no possible departure but
                    // must never clear an incoming true fact.
                    donor_moved |=
                        !matches!(amount, ValueExpr::Literal(value) if *value == 0.into());
                    family
                }
                Instruction::DiscardOneAtRandom(player) => {
                    if !matches!(
                        player,
                        ResolvingPlayer::Controller | ResolvingPlayer::TargetPlayer
                    ) {
                        return Err(DefinitionError::InvalidInstruction);
                    }
                    facts.definitely_discarded = true;
                    player.validate(context, self.target, facts.definitely_exiled_target)?
                }
                Instruction::SacrificeRandomUntilLand => {
                    if self.target != BodyTarget::SingleOpponent {
                        return Err(DefinitionError::InvalidInstruction);
                    }
                    Some(OperandFamily::Player)
                }
                Instruction::DiscardUpTo { maximum } => {
                    if *maximum == 0 || self.target != BodyTarget::None {
                        return Err(DefinitionError::InvalidInstruction);
                    }
                    facts.definitely_discarded = true;
                    None
                }
                Instruction::SacrificeMatching(filter) => {
                    if self.target != BodyTarget::None
                        || matches!(filter, SacrificeSelection::Subtype(kind) if vocabulary.label(*kind).is_none())
                    {
                        return Err(DefinitionError::InvalidInstruction);
                    }
                    facts.definitely_sacrificed = true;
                    donor_moved = true;
                    None
                }
                Instruction::SacrificeToContinue { filter } => {
                    if index != 0 || self.instructions.len() < 2 {
                        return Err(DefinitionError::InvalidInstruction);
                    }
                    filter.validate()?;
                    None
                }
                Instruction::Discard { players, amount } => {
                    let family =
                        players.validate(context, self.target, facts.definitely_exiled_target)?;
                    amount.validate_amount(&defined_slots, context, &facts, vocabulary)?;
                    facts.definitely_discarded = true;
                    family
                }
                Instruction::CounterSpell | Instruction::CounterSpellToExile => {
                    if matches!(instruction, Instruction::CounterSpellToExile)
                        && !matches!(self.target, BodyTarget::SingleSpell(_))
                    {
                        return Err(DefinitionError::OperandFamily);
                    }
                    Some(
                        if self.target == BodyTarget::StackEntryTargetingControlledPermanent {
                            OperandFamily::StackEntry
                        } else {
                            OperandFamily::Spell
                        },
                    )
                }
                Instruction::LinkedPlayerLosesDamageAmount => {
                    if self.target != BodyTarget::None
                        || !matches!(
                            context,
                            BodyContext::TriggerBound {
                                condition: TriggerSpec {
                                    event: TriggerEvent::DealtDamage {
                                        linked_player: true,
                                        ..
                                    },
                                    ..
                                },
                                grouping: TriggerGrouping::OneOrMoreByPlayer,
                            }
                        )
                    {
                        return Err(DefinitionError::InvalidInstruction);
                    }
                    None
                }
                Instruction::RelatedPlayerLosesHalfLife => {
                    if !matches!(context, BodyContext::TriggerBound {
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
                    } if *source == ObjectFilter::any().attached_to_self() || *source == ObjectFilter::self_object())
                    {
                        return Err(DefinitionError::InvalidInstruction);
                    }
                    None
                }
                Instruction::PutCounterOnRelatedPermanent => {
                    let valid = matches!(
                        context,
                        BodyContext::TriggerBound {
                            condition,
                            grouping: TriggerGrouping::EachOccurrence,
                        } if match &condition.event {
                            TriggerEvent::Enters {
                                from_zone: TriggerFromZone::Any,
                                cast_origin: None,
                                attached_to_creature: false,
                                ..
                            } => condition.intervening_if.is_none(),
                            TriggerEvent::Attacks {
                                subject,
                                mode: AttackTriggerMode::Individual { capture_defender: false },
                            } => condition.intervening_if.is_none()
                                && (*subject == ObjectFilter::self_object()
                                    || *subject == ObjectFilter::any().attached_to_self()
                                    || *subject == ObjectFilter::any().controlled().commander()),
                            TriggerEvent::DealsDamage {
                                recipient: DamageTriggerRecipient::EnchantedPlayer,
                                combat_only: true,
                                ..
                            } => true,
                            TriggerEvent::DealsDamage {
                                source: ObjectFilter::Clause(c),
                                recipient: DamageTriggerRecipient::AnyPlayer,
                                combat_only: true,
                            } => c.controller == ControllerFilter::You
                                && matches!(c.subtypes, SubtypeFilter::Has(_)),
                            _ => false,
                        }
                    );
                    if !valid || self.target != BodyTarget::None {
                        return Err(DefinitionError::InvalidInstruction);
                    }
                    None
                }
                Instruction::PutCountersOnControlledCreatures
                | Instruction::PutCountersOnOtherCreatures
                | Instruction::PutCreatureAndOtherPlaneswalkerCounters
                | Instruction::PutChargeCounterOnSource
                | Instruction::PutBlessingCounterOnSource
                | Instruction::PutQuestCounterOnSource
                | Instruction::PutStorageCounterOnSource
                | Instruction::ReturnSourceToHand
                | Instruction::UntapAbilitySource
                | Instruction::TapAbilitySource
                | Instruction::RegenerateSource => None,
                Instruction::ExileDefeatedSiege | Instruction::CastDefeatedSiege => {
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
                                Instruction::ExileDefeatedSiege,
                                Instruction::CastDefeatedSiege,
                            ]
                    {
                        return Err(DefinitionError::InvalidInstruction);
                    }
                    None
                }
                Instruction::ExileAbilitySource => {
                    if self.target != BodyTarget::None {
                        return Err(DefinitionError::OperandFamily);
                    }
                    donor_moved = true;
                    None
                }
                Instruction::ExileSourceAndGainLife(n) => {
                    if *n == 0
                        || !matches!(context,BodyContext::TriggerBound {
                        condition: TriggerSpec {
                            event: TriggerEvent::StateThreshold(StateThresholdKind::SourceCounters {
                                kind: crate::effects::CounterKind::Page,
                                comparison: Comparison::GreaterOrEqual,
                                amount: minimum,
                            }),
                            ..
                        },
                        grouping: TriggerGrouping::EachOccurrence,
                    } if *minimum > 0)
                    {
                        return Err(DefinitionError::InvalidInstruction);
                    }
                    None
                }
                Instruction::FightSourceWithTarget => {
                    if self.target != BodyTarget::SingleCreature {
                        return Err(DefinitionError::OperandFamily);
                    }
                    Some(OperandFamily::Permanent)
                }
                Instruction::ClearSourceCounters(kind) => {
                    if self.target != BodyTarget::None
                        || !matches!(context, BodyContext::Activated { .. })
                        || *kind != crate::effects::CounterKind::Hatchling
                    {
                        return Err(DefinitionError::InvalidInstruction);
                    }
                    None
                }
                Instruction::TransformSource => {
                    if self.target != BodyTarget::None {
                        return Err(DefinitionError::OperandFamily);
                    }
                    None
                }
                Instruction::UntapControlledCreatures
                | Instruction::TapAttackersAndSkipNextUntap => {
                    if self.target != BodyTarget::None {
                        return Err(DefinitionError::OperandFamily);
                    }
                    None
                }
                Instruction::RegenerateTarget | Instruction::GoadTarget => {
                    Some(OperandFamily::Permanent)
                }
                Instruction::GoadOtherPlayersCreatures => None,
                Instruction::ShuffleAbilitySourceIntoLibrary
                    if self.target == BodyTarget::None
                        && matches!(
                            context,
                            BodyContext::Activated { .. } | BodyContext::TriggerBound { .. }
                        ) =>
                {
                    None
                }
                Instruction::RevealToNameAfterSourceShuffle(_)
                    if self.target == BodyTarget::None
                        && self.instructions[..index]
                            .contains(&Instruction::ShuffleAbilitySourceIntoLibrary) =>
                {
                    None
                }
                Instruction::ShuffleAbilitySourceIntoLibrary
                | Instruction::RevealToNameAfterSourceShuffle(_) => {
                    return Err(DefinitionError::InvalidInstruction);
                }
                Instruction::ChooseMatchingHandSpellToCast
                    if self.instructions[..index].iter().any(|prior| {
                        matches!(
                            prior,
                            Instruction::CounterSpell | Instruction::CounterSpellToExile
                        )
                    }) =>
                {
                    None
                }
                Instruction::ChooseMatchingHandSpellToCast => {
                    return Err(DefinitionError::InvalidInstruction);
                }
                Instruction::GainControlOfSpell => Some(OperandFamily::Spell),
                Instruction::ChooseNewTargets => {
                    Some(if matches!(self.target, BodyTarget::SingleSpell(_)) {
                        OperandFamily::Spell
                    } else {
                        OperandFamily::StackEntry
                    })
                }
                Instruction::ChangeOnlyTarget
                | Instruction::LoseLifeEqualToTargetSpellManaValue => {
                    if self.target != BodyTarget::SingleSpell(SpellFilter::any().single_target()) {
                        return Err(DefinitionError::InvalidInstruction);
                    }
                    Some(OperandFamily::Spell)
                }
                Instruction::CopyAbility
                    if matches!(self.target, BodyTarget::ControlledAbility { .. }) =>
                {
                    Some(OperandFamily::StackEntry)
                }
                Instruction::CopyAbility => return Err(DefinitionError::InvalidInstruction),
                Instruction::CopySpell => Some(OperandFamily::Spell),
                Instruction::CopyCreatureAsToken => {
                    if self.target
                        != BodyTarget::FilteredPermanent(ObjectFilter::creature().controlled())
                    {
                        return Err(DefinitionError::InvalidInstruction);
                    }
                    Some(OperandFamily::Permanent)
                }
                Instruction::CopyTargetToSource { .. } => {
                    // CR 608.2h would require the departed donor's copiable LKI.
                    // Do not admit that composition until such capture exists.
                    if donor_moved {
                        return Err(DefinitionError::InvalidInstruction);
                    }
                    Some(OperandFamily::Permanent)
                }
                Instruction::DamageFromPower(source) => {
                    if *source == PowerDamageSource::TargetCreatureToAbilitySource
                        && !matches!(
                            context,
                            BodyContext::Activated { .. } | BodyContext::TriggerBound { .. }
                        )
                    {
                        return Err(DefinitionError::InvalidInstruction);
                    }
                    if matches!(
                        source,
                        PowerDamageSource::TargetCreature
                            | PowerDamageSource::TargetCreatureToAbilitySource
                    ) && !self.target.guarantees_creature()
                    {
                        return Err(DefinitionError::OperandFamily);
                    }
                    if *source == PowerDamageSource::GraveyardEntrant
                        && !matches!(context, BodyContext::TriggerBound {
                        condition: TriggerSpec {
                            event: TriggerEvent::Enters {
                                subject,
                                from_zone: TriggerFromZone::ControllerGraveyard,
                                ..
                            },
                            ..
                        },
                        grouping: TriggerGrouping::EachOccurrence,
                    } if *subject == ObjectFilter::creature())
                    {
                        return Err(DefinitionError::InvalidInstruction);
                    }
                    if *source == PowerDamageSource::CreatureDealtDamage
                        && !matches!(context, BodyContext::TriggerBound {
                        condition: TriggerSpec {
                            event: TriggerEvent::DealtDamage { subject, linked_player: false },
                            ..
                        },
                        grouping: TriggerGrouping::OneOrMoreTogether,
                    } if *subject == ObjectFilter::any().attached_to_self())
                    {
                        return Err(DefinitionError::InvalidInstruction);
                    }
                    Some(OperandFamily::DamageRecipient)
                }
                Instruction::DamageCreatureAtSpellMastery { amount, minimum } => {
                    if *minimum == 0 || self.target != BodyTarget::SingleCreature {
                        return Err(DefinitionError::InvalidInstruction);
                    }
                    amount.validate_amount(&defined_slots, context, &facts, vocabulary)?;
                    Some(OperandFamily::DamageRecipient)
                }
                Instruction::DamageAmount(amount)
                | Instruction::DamageAmountThenExileIfDies(amount) => {
                    if amount.is_target_player_hand_size()
                        && !matches!(
                            self.target,
                            BodyTarget::SinglePlayer | BodyTarget::SingleOpponent
                        )
                    {
                        return Err(DefinitionError::OperandFamily);
                    }
                    amount.validate_amount(&defined_slots, context, &facts, vocabulary)?;
                    Some(OperandFamily::DamageRecipient)
                }
                Instruction::Damage { amount } => {
                    if !amount.is_positive() {
                        return Err(DefinitionError::InvalidInstruction);
                    }
                    Some(OperandFamily::DamageRecipient)
                }
                Instruction::CreateReflectingShield => {
                    if self.target != BodyTarget::SingleAnyTarget
                        || !matches!(context, BodyContext::Unbound)
                    {
                        return Err(DefinitionError::InvalidInstruction);
                    }
                    Some(OperandFamily::DamageRecipient)
                }
                Instruction::CreatePreventionShield { amount } => {
                    if !amount.is_positive() {
                        return Err(DefinitionError::InvalidInstruction);
                    }
                    Some(OperandFamily::DamageRecipient)
                }
                Instruction::CreateDamageMultiplier { factor } => {
                    if !factor.is_positive() {
                        return Err(DefinitionError::InvalidInstruction);
                    }
                    Some(OperandFamily::Permanent)
                }
                Instruction::GrantProtectionChosenColor(ProtectionRecipients::TargetPermanent)
                | Instruction::PutIndestructibleCounter
                | Instruction::PutLoyaltyCounter => Some(OperandFamily::Permanent),
                Instruction::GrantProtectionChosenColor(
                    ProtectionRecipients::ControllerAndPermanents,
                ) => None,
                Instruction::PutPowerToughnessCounters { counters } => {
                    if !counters.valid_placement() {
                        return Err(DefinitionError::InvalidInstruction);
                    }
                    Some(OperandFamily::Permanent)
                }
                Instruction::AnimateTargetAsCountedCreature {
                    amount,
                    subtype,
                    keywords,
                    ..
                } => {
                    amount.validate_amount(&defined_slots, context, &facts, vocabulary)?;
                    if vocabulary.label(*subtype).is_none() {
                        return Err(DefinitionError::ForeignCreatureType);
                    }
                    let parts = ResolvedPartsSpec(vec![
                        crate::characteristics::ResolvedPartSpec::Abilities(
                            crate::characteristics::AbilityChange::grant_keywords(
                                keywords.iter().copied(),
                            ),
                        ),
                    ]);
                    parts
                        .validate(vocabulary, &defined_slots)
                        .map_err(|_| DefinitionError::ContinuousEffect)?;
                    Some(OperandFamily::Permanent)
                }
                Instruction::WeakenTargetByChosenX => {
                    if self.target != BodyTarget::SingleCreature {
                        return Err(DefinitionError::InvalidInstruction);
                    }
                    ValueExpr::chosen_x().validate_amount(
                        &defined_slots,
                        context,
                        &facts,
                        vocabulary,
                    )?;
                    Some(OperandFamily::Permanent)
                }
                Instruction::BoostTargetPower {
                    amount, keywords, ..
                } => {
                    if matches!(
                        amount,
                        ValueExpr::ObjectStat {
                            subject: ValueObjectRef::TargetObject,
                            stat: ObjectStatKind::Power
                        }
                    ) && self.target != BodyTarget::SingleCreature
                    {
                        return Err(DefinitionError::OperandFamily);
                    }
                    amount.validate_amount(&defined_slots, context, &facts, vocabulary)?;
                    let parts = ResolvedPartsSpec(vec![
                        crate::characteristics::ResolvedPartSpec::Abilities(
                            crate::characteristics::AbilityChange::grant_keywords(
                                keywords.iter().copied(),
                            ),
                        ),
                    ]);
                    parts
                        .validate(vocabulary, &defined_slots)
                        .map_err(|_| DefinitionError::ContinuousEffect)?;
                    Some(OperandFamily::Permanent)
                }
                Instruction::CreateContinuousEffect { parts, duration } => {
                    if matches!(duration, ResolvedDuration::WhileSourceControlled { .. })
                        || (*duration == ResolvedDuration::WhileControllingAbilitySource
                            && !matches!(
                                context,
                                BodyContext::Activated { .. } | BodyContext::TriggerBound { .. }
                            ))
                    {
                        return Err(DefinitionError::InvalidInstruction);
                    }
                    parts
                        .validate(vocabulary, &defined_slots)
                        .map_err(|error| match error {
                            CharacteristicDefinitionError::UndefinedCreatureTypeSlot => {
                                DefinitionError::UndefinedCreatureTypeSlot
                            }
                            _ => DefinitionError::ContinuousEffect,
                        })?;
                    Some(OperandFamily::Permanent)
                }
                Instruction::ResolveKeywordTrigger(keyword) => {
                    if matches!(keyword, CombatTriggerKeyword::Mobilize(_))
                        && vocabulary.resolve_label("Warrior").is_none()
                    {
                        return Err(DefinitionError::TokenDefinition);
                    }
                    if !matches!(context, BodyContext::TriggerBound { condition, grouping: TriggerGrouping::EachOccurrence }
                        if *condition == keyword.condition())
                        || !keyword.valid()
                    {
                        return Err(DefinitionError::InvalidInstruction);
                    }
                    None
                }
                Instruction::Ninjutsu | Instruction::Unearth => {
                    if self.target != BodyTarget::None {
                        return Err(DefinitionError::InvalidInstruction);
                    }
                    None
                }
                Instruction::Cascade | Instruction::CascadeBottom => {
                    if !matches!(
                        context,
                        BodyContext::TriggerBound {
                            condition,
                            grouping: TriggerGrouping::EachOccurrence
                        } if *condition == TriggerSpec::self_cast()
                    ) || self.target != BodyTarget::None
                        || self.instructions != [Instruction::Cascade, Instruction::CascadeBottom]
                    {
                        return Err(DefinitionError::InvalidInstruction);
                    }
                    None
                }
                Instruction::Myriad
                    if self.target == BodyTarget::None
                        && matches!(context,BodyContext::TriggerBound {condition: TriggerSpec { event: TriggerEvent::Attacks { subject, mode: AttackTriggerMode::Individual { capture_defender: true } }, .. },grouping:TriggerGrouping::EachOccurrence} if *subject == ObjectFilter::self_object()) =>
                {
                    None
                }
                Instruction::Myriad => return Err(DefinitionError::InvalidInstruction),
                Instruction::CopyForOtherCreatures
                    if self.target == BodyTarget::None
                        && matches!(context,BodyContext::TriggerBound {condition: TriggerSpec { event: TriggerEvent::SpellCast { caster: TriggerPlayerScope::Relation(PlayerRelation::Controller), filter, self_cast: false, .. }, .. },grouping:TriggerGrouping::EachOccurrence} if *filter == SpellFilter::instant_or_sorcery().targeting_only_source()) =>
                {
                    None
                }
                Instruction::CopyForOtherCreatures => {
                    return Err(DefinitionError::InvalidInstruction);
                }
                Instruction::WatchNextSpellOrLoyaltyTwice
                    if self.target == BodyTarget::None
                        && matches!(context, BodyContext::Unbound) =>
                {
                    None
                }
                Instruction::WatchNextSpellOrLoyaltyTwice => {
                    return Err(DefinitionError::InvalidInstruction);
                }
                Instruction::CopyResolvingSpell if matches!(context, BodyContext::Unbound) => None,
                Instruction::CopyResolvingSpell => return Err(DefinitionError::InvalidInstruction),
                Instruction::WatchNextLoyaltyCopy
                | Instruction::ChoosePlaneswalkerTypeForCopies
                    if self.target == BodyTarget::None
                        && matches!(context, BodyContext::TriggerBound { .. }) =>
                {
                    None
                }
                Instruction::WatchNextLoyaltyCopy
                | Instruction::ChoosePlaneswalkerTypeForCopies
                | Instruction::CopyCapturedAbility => {
                    return Err(DefinitionError::InvalidInstruction);
                }
                Instruction::CopyOnCast(kind) => {
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
                Instruction::ExileCreatureAtMostRevealedPower
                | Instruction::ExileCreaturesAndPlaneswalkersBelowRevealedManaValue => {
                    let expected =
                        if matches!(instruction, Instruction::ExileCreatureAtMostRevealedPower) {
                            BodyTarget::SingleCreature
                        } else {
                            BodyTarget::None
                        };
                    if !matches!(context, BodyContext::RevealedCreature) || self.target != expected
                    {
                        return Err(DefinitionError::InvalidInstruction);
                    }
                    (expected == BodyTarget::SingleCreature).then_some(OperandFamily::Permanent)
                }
                Instruction::CastLinkedSpellMatchingX
                | Instruction::DestroyDamagedPlayersNonlandsAtX
                | Instruction::PutChosenXChargeCounters
                | Instruction::DestroyNonlandsAtSourceChargeCount => {
                    if self.target != BodyTarget::None || !matches!(context, BodyContext::Unbound) {
                        return Err(DefinitionError::InvalidInstruction);
                    }
                    None
                }
                Instruction::AddCounterAndDischargeOnResolution(ordinal) => {
                    if *ordinal == 0
                        || self.target != BodyTarget::None
                        || !matches!(context, BodyContext::Unbound)
                    {
                        return Err(DefinitionError::InvalidInstruction);
                    }
                    None
                }
                Instruction::ReturnOwnedCommandersToHand
                | Instruction::ExileTopForFreePlay
                | Instruction::ShuffleEachHandAndGraveyard
                | Instruction::EndControllerTurn
                | Instruction::ShuffleOwnLibrary
                | Instruction::ShuffleOwnGraveyardIntoLibrary
                | Instruction::ExileAllGraveyards => None,
                Instruction::ShuffleTargetGraveyardCards { maximum }
                    if *maximum > 0
                        && self.target == BodyTarget::SinglePlayer
                        && self.instructions.len() == 1
                        && matches!(context, BodyContext::Unbound) =>
                {
                    Some(OperandFamily::Player)
                }
                Instruction::ShuffleTargetGraveyardCards { .. } => {
                    return Err(DefinitionError::InvalidInstruction);
                }
                Instruction::ReturnSourceGraveyardToBattlefield { .. }
                    if self.target == BodyTarget::None
                        && matches!(context,BodyContext::TriggerBound {condition: TriggerSpec { event: TriggerEvent::SpellCast { caster: TriggerPlayerScope::Relation(PlayerRelation::Controller), filter, self_cast: false, .. }, zone: TriggerZone::Graveyard, .. },grouping:TriggerGrouping::EachOccurrence} if *filter == SpellFilter::of_type(CardType::Planeswalker)) =>
                {
                    None
                }
                Instruction::ReturnSourceGraveyardToBattlefield { .. }
                    if matches!(
                        context,
                        BodyContext::Unbound
                            | BodyContext::Activated { .. }
                            | BodyContext::SacrificedPermanent
                            | BodyContext::RevealedCreature
                            | BodyContext::ExiledCreatureCost
                            | BodyContext::SacrificedCreature
                    ) && self.target == BodyTarget::None
                        && self.instructions.len() == 1 =>
                {
                    None
                }
                Instruction::ExileUpToTwoGraveyardTargetsGainLife
                    if self.target == BodyTarget::AnyGraveyardCard
                        && self.instructions.len() == 1 =>
                {
                    Some(OperandFamily::GraveyardCard)
                }
                Instruction::ExileUpToTwoGraveyardTargetsGainLife => {
                    return Err(DefinitionError::InvalidInstruction);
                }
                Instruction::ReturnUpToOneRecentGraveyardCard => {
                    if self.target != BodyTarget::RecentlyDiscardedOrFromLibrary
                        || self.instructions.len() != 1
                    {
                        return Err(DefinitionError::InvalidInstruction);
                    }
                    Some(OperandFamily::GraveyardCard)
                }
                Instruction::ReturnRandomGraveyardCards {
                    filter,
                    to_battlefield,
                    ..
                } => {
                    if self.target != BodyTarget::None
                        || (*to_battlefield && filter.as_deref() != Some(&ObjectFilter::creature()))
                    {
                        return Err(DefinitionError::InvalidInstruction);
                    }
                    if let Some(filter) = filter {
                        filter
                            .validate_vocabulary(vocabulary)
                            .map_err(|_| DefinitionError::InvalidInstruction)?;
                    }
                    None
                }
                Instruction::ReturnTargetGraveyardCreatureToHand => {
                    if !matches!(self.target, BodyTarget::FilteredOwnGraveyardCard(_)) {
                        return Err(DefinitionError::InvalidInstruction);
                    }
                    Some(OperandFamily::GraveyardCard)
                }
                Instruction::ChooseRevealedHandCard { .. } => {
                    if !matches!(
                        self.target,
                        BodyTarget::SingleOpponent | BodyTarget::SinglePlayer
                    ) {
                        return Err(DefinitionError::InvalidInstruction);
                    }
                    Some(OperandFamily::Player)
                }
                Instruction::ReturnUpToOneOwnGraveyardCreatureToHand
                    if self.target == BodyTarget::None =>
                {
                    None
                }
                Instruction::ReturnUpToOneOwnGraveyardCreatureToHand => {
                    return Err(DefinitionError::InvalidInstruction);
                }
                Instruction::ReturnSourceGraveyardToHand
                    if self.target == BodyTarget::None
                        && (matches!(context, BodyContext::TriggerBound { condition, grouping: TriggerGrouping::EachOccurrence } if *condition == TriggerSpec::step_began(Step::Upkeep, PlayerRelation::Controller).in_zone(TriggerZone::Graveyard))
                            || matches!(
                                context,
                                BodyContext::Unbound
                                    | BodyContext::Activated { .. }
                                    | BodyContext::SacrificedPermanent
                                    | BodyContext::RevealedCreature
                                    | BodyContext::ExiledCreatureCost
                                    | BodyContext::SacrificedCreature
                            ) && self.instructions.len() == 1) =>
                {
                    None
                }
                Instruction::ReturnSourceGraveyardToBattlefield { .. }
                | Instruction::ReturnSourceGraveyardToHand => {
                    return Err(DefinitionError::InvalidInstruction);
                }
                Instruction::BecomeMonarch(None) => None,
                Instruction::TapEnchantedCreature => {
                    if self.target != BodyTarget::None
                        || !matches!(
                            context,
                            BodyContext::TriggerBound { .. } | BodyContext::Activated { .. }
                        )
                    {
                        return Err(DefinitionError::InvalidInstruction);
                    }
                    None
                }
                Instruction::BecomeMonarch(Some(_)) => {
                    return Err(DefinitionError::InvalidInstruction);
                }
                Instruction::PayControllerLife(n) => {
                    if *n == 0
                        || !index
                            .checked_sub(1)
                            .and_then(|i| self.instructions.get(i))
                            .is_some_and(|previous| {
                                matches!(
                                    previous,
                                    Instruction::OptionalSequence {
                                        chooser: ResolvingPlayer::Controller,
                                        ..
                                    }
                                )
                            })
                    {
                        return Err(DefinitionError::InvalidInstruction);
                    }
                    None
                }
                Instruction::PutEquipmentFromHandOrGraveyard => {
                    if self.target != BodyTarget::None {
                        return Err(DefinitionError::InvalidInstruction);
                    }
                    None
                }
                Instruction::Encore => {
                    if !matches!(context, BodyContext::Unbound)
                        || self.target != BodyTarget::None
                        || self.instructions.len() != 1
                    {
                        return Err(DefinitionError::InvalidInstruction);
                    }
                    None
                }
                Instruction::EquipNewToken | Instruction::GrantNewTokensHaste => {
                    if !index
                        .checked_sub(1)
                        .and_then(|i| self.instructions.get(i))
                        .is_some_and(|op| {
                            matches!(
                                op,
                                Instruction::CreateTokens { .. }
                                    | Instruction::CreateAttackingTokens { .. }
                            )
                        })
                    {
                        return Err(DefinitionError::InvalidInstruction);
                    }
                    None
                }
                Instruction::RequireCondition(cond) => {
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
                Instruction::TapTargetWhileControllingSource
                    if self.target
                        == BodyTarget::FilteredPermanent(ObjectFilter::creature().opposing())
                        && matches!(context, BodyContext::TriggerBound { .. }) =>
                {
                    Some(OperandFamily::Permanent)
                }
                Instruction::TapTargetWhileControllingSource => {
                    return Err(DefinitionError::InvalidInstruction);
                }
                Instruction::RemoveTargetFromCombat
                    if self.target
                        == BodyTarget::FilteredPermanent(
                            ObjectFilter::creature().attacking_or_blocking(),
                        ) =>
                {
                    Some(OperandFamily::Permanent)
                }
                Instruction::RemoveTargetFromCombat => return Err(DefinitionError::OperandFamily),
                Instruction::RegenerateControlledSubtype(kind) => {
                    if self.target != BodyTarget::None {
                        return Err(DefinitionError::OperandFamily);
                    }
                    if vocabulary.label(*kind).is_none() {
                        return Err(DefinitionError::ForeignCreatureType);
                    }
                    None
                }
                Instruction::RequireTargetAttack if self.target.guarantees_creature() => {
                    Some(OperandFamily::Permanent)
                }
                Instruction::RequireTargetAttack => return Err(DefinitionError::OperandFamily),
                Instruction::RequireAttackSourceNextTurn
                    if self.target == BodyTarget::SingleOpponent =>
                {
                    Some(OperandFamily::Player)
                }
                Instruction::RequireAttackSourceNextTurn => {
                    return Err(DefinitionError::OperandFamily);
                }
                Instruction::ChooseAttackDirection | Instruction::AdditionalLoyaltyActivation => {
                    None
                }
                Instruction::MoveLoyaltyCounterToEnteringPermanent => {
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
                    {
                        return Err(DefinitionError::InvalidInstruction);
                    }
                    None
                }
                Instruction::AbilitySourceControllerLosesLife(amount) => {
                    if *amount == 0 || !matches!(context, BodyContext::Activated { .. }) {
                        return Err(DefinitionError::InvalidInstruction);
                    }
                    None
                }
                Instruction::RelatedPlayerLosesLife(amount) => {
                    if *amount == 0
                        || !matches!(
                            context,
                            BodyContext::TriggerBound {
                                condition: TriggerSpec {
                                    intervening_if: Some(TriggerCondition::SourceCounterAtLeast {
                                        kind: crate::effects::CounterKind::Quest,
                                        ..
                                    }),
                                    ..
                                },
                                grouping: TriggerGrouping::EachOccurrence
                            }
                        )
                    {
                        return Err(DefinitionError::InvalidInstruction);
                    }
                    None
                }
                Instruction::IncreaseSpeed
                | Instruction::EndStepCast { .. }
                | Instruction::MadnessCast
                | Instruction::MiracleCast(_)
                | Instruction::ReboundCast(_)
                | Instruction::ExileDelayedObjects(_)
                | Instruction::SacrificeDelayedObjects(_)
                | Instruction::ScheduleArtifactReturn(_)
                | Instruction::ReturnDelayedGraveyardToHand(_)
                | Instruction::ReturnDelayedGraveyardCard(_)
                | Instruction::ReturnDelayedGraveyardCardTransformed(_)
                | Instruction::ChooseFoundSpell
                | Instruction::CastFoundSpell(_)
                | Instruction::ReturnDelayedPermanentToHand(_)
                | Instruction::ReturnDelayedExileWithCounters(_)
                | Instruction::ReturnDelayedExile(_)
                | Instruction::ExileUnearthed(_) => {
                    return Err(DefinitionError::InvalidInstruction);
                }
                Instruction::DrainOpponents(amount) => {
                    amount.validate_amount(&defined_slots, context, &facts, vocabulary)?;
                    None
                }
                Instruction::ExtortDrain => {
                    let definition = TriggeredAbility::extort(TriggeredAbilityId(0));
                    if !matches!(context,BodyContext::TriggerBound{condition,grouping:TriggerGrouping::EachOccurrence} if *condition==definition.condition)
                        || *self != definition.body
                    {
                        return Err(DefinitionError::InvalidInstruction);
                    }
                    None
                }
                Instruction::ResolveTransfer(keyword) => {
                    if !matches!(context,BodyContext::TriggerBound{condition,grouping:TriggerGrouping::EachOccurrence} if *condition==keyword.condition())
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
                Instruction::ResolveGrowth(keyword) => {
                    if !matches!(context,BodyContext::TriggerBound{condition,grouping:TriggerGrouping::EachOccurrence} if *condition==keyword.condition())
                        || self.target != keyword.target()
                        || self.instructions.len() != 1
                        || matches!(keyword,GrowthKeyword::Backup{amount,..} if amount < &0.into())
                    {
                        return Err(DefinitionError::InvalidInstruction);
                    }
                    (self.target != BodyTarget::None).then_some(OperandFamily::Permanent)
                }
                Instruction::ResolveCounterKeyword(keyword) => {
                    if !matches!(context,BodyContext::TriggerBound{condition,grouping:TriggerGrouping::EachOccurrence} if *condition==TriggerSpec::counter_keyword(*keyword))
                        || self.instructions.len() != 1
                        || self.target != BodyTarget::None
                    {
                        return Err(DefinitionError::InvalidInstruction);
                    }
                    None
                }
                Instruction::SacrificeStateTriggerSource => {
                    if self.instructions.first() == Some(&Instruction::OfferOpponentsDamage) {
                        None
                    } else if self.target != BodyTarget::None
                        || !matches!(
                            context,
                            BodyContext::TriggerBound {
                                grouping: TriggerGrouping::EachOccurrence,
                                ..
                            }
                        )
                    {
                        return Err(DefinitionError::InvalidInstruction);
                    } else {
                        None
                    }
                }
                Instruction::SacrificeEvoked => {
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
                Instruction::SacrificeUpkeepSource(keyword) => {
                    if self.target != BodyTarget::None
                        || !matches!(context, BodyContext::TriggerBound {
                            condition: TriggerSpec {
                                event: TriggerEvent::Keyword(TriggerKeywordEvent::Upkeep(kind)),
                                ..
                            },
                            grouping: TriggerGrouping::EachOccurrence,
                        } if kind == keyword)
                        || !matches!(
                            self.instructions.as_slice(),
                            [
                                Instruction::OfferManaPayment {
                                    payer: ResolvingPlayer::Controller,
                                    paid_destination: 2,
                                    declined_destination: 1,
                                    ..
                                },
                                Instruction::SacrificeUpkeepSource(_)
                            ]
                        )
                    {
                        return Err(DefinitionError::InvalidInstruction);
                    }
                    None
                }
                Instruction::CounterWardSource => {
                    if !matches!(
                        context,
                        BodyContext::TriggerBound {
                            condition: TriggerSpec {
                                event: TriggerEvent::Targeted {
                                    by_opponent: true,
                                    spell_only: false,
                                    ..
                                },
                                ..
                            },
                            grouping: TriggerGrouping::EachOccurrence,
                        }
                    ) {
                        return Err(DefinitionError::InvalidInstruction);
                    }
                    None
                }
                Instruction::BoostSourceByCastManaValue => {
                    if !matches!(context, BodyContext::TriggerBound {
                        condition: TriggerSpec {
                            event: TriggerEvent::SpellCast { filter, self_cast: false, .. },
                            ..
                        },
                        grouping: TriggerGrouping::EachOccurrence,
                    } if *filter == SpellFilter::colorless())
                        || self.target != BodyTarget::None
                    {
                        return Err(DefinitionError::InvalidInstruction);
                    }
                    None
                }
                Instruction::BoostLoneAttacker => {
                    if !matches!(
                        context,
                        BodyContext::TriggerBound {
                            condition: TriggerSpec {
                                event: TriggerEvent::Attacks {
                                    mode: AttackTriggerMode::Alone,
                                    ..
                                },
                                ..
                            },
                            grouping: TriggerGrouping::EachOccurrence,
                        }
                    ) {
                        return Err(DefinitionError::InvalidInstruction);
                    }
                    None
                }
                Instruction::SetControlledCreatureSizeToHand if self.target != BodyTarget::None => {
                    return Err(DefinitionError::InvalidInstruction);
                }
                Instruction::SetControlledCreatureSizeToHand => None,
                Instruction::ModifyGroupByAmount { amount, group, .. } => {
                    if matches!(
                        group.as_ref(),
                        EffectSubject::TriggeringAttacker
                            | EffectSubject::TriggeringAttackGroup
                            | EffectSubject::TriggeringTarget
                    ) {
                        return Err(DefinitionError::InvalidInstruction);
                    }
                    group
                        .validate_vocabulary(vocabulary)
                        .map_err(|_| DefinitionError::InvalidInstruction)?;
                    amount.validate_amount(&defined_slots, context, &facts, vocabulary)?;
                    None
                }
                Instruction::ModifyGroupUntilEndOfTurn {
                    group,
                    power_toughness,
                    keywords,
                    ..
                } => {
                    if matches!(group.as_ref(), EffectSubject::TriggeringTarget)
                        && !matches!(
                            context,
                            BodyContext::TriggerBound {
                                condition: TriggerSpec {
                                    event: TriggerEvent::Targeted {
                                        instant_or_sorcery_only: true,
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
                    if matches!(group.as_ref(), EffectSubject::TriggeringAttackGroup)
                        && !matches!(
                            context,
                            BodyContext::TriggerBound {
                                condition: TriggerSpec {
                                    event: TriggerEvent::Attacks {
                                        mode: AttackTriggerMode::AtOpponentOrPlaneswalker,
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
                    if matches!(group.as_ref(), EffectSubject::TriggeringAttacker)
                        && !matches!(context, BodyContext::TriggerBound {
                            condition: TriggerSpec { event, .. },
                            grouping: TriggerGrouping::EachOccurrence,
                        } if matches!(
                            event,
                            TriggerEvent::Attacks { mode: AttackTriggerMode::Alone | AttackTriggerMode::AtControllerOrPlaneswalker, .. }
                        ) || matches!(
                            event,
                            TriggerEvent::Attacks { subject, mode: AttackTriggerMode::Individual { capture_defender: false } }
                                if *subject == ObjectFilter::creature().controlled().other_than_ability_source()
                                    || matches!(subject, ObjectFilter::Clause(c) if c.token == Some(true))
                        ))
                    {
                        return Err(DefinitionError::InvalidInstruction);
                    }
                    group
                        .validate_vocabulary(vocabulary)
                        .map_err(|_| DefinitionError::InvalidInstruction)?;
                    if power_toughness.is_none() && keywords.is_empty() {
                        return Err(DefinitionError::InvalidInstruction);
                    }
                    None
                }
                Instruction::Adapt(amount) => {
                    if *amount == 0
                        || self.target != BodyTarget::None
                        || self.instructions.len() != 1
                        || !matches!(
                            context,
                            BodyContext::Unbound | BodyContext::Activated { .. }
                        )
                    {
                        return Err(DefinitionError::InvalidInstruction);
                    }
                    None
                }
                Instruction::BoostSource {
                    power_toughness,
                    keywords,
                } => {
                    if !matches!(
                        context,
                        BodyContext::Unbound
                            | BodyContext::Activated { .. }
                            | BodyContext::SacrificedPermanent
                            | BodyContext::RevealedCreature
                            | BodyContext::ExiledCreatureCost
                            | BodyContext::SacrificedCreature
                    ) && !matches!(context, BodyContext::TriggerBound {
                            condition: TriggerSpec {
                                event: TriggerEvent::CountersChanged {
                                    subject,
                                    kind: CounterTriggerKind::PlusOnePlaced { by_controller_only: false },
                                },
                                ..
                            },
                            grouping: TriggerGrouping::EachOccurrence,
                        } if *subject == ObjectFilter::self_object())
                        || self.target != BodyTarget::None
                        || self.instructions.len() != 1
                        || (power_toughness.is_none() && keywords.is_empty())
                    {
                        return Err(DefinitionError::InvalidInstruction);
                    }
                    None
                }
                Instruction::UnattachEquipmentAndProtectHost => {
                    if self.target != BodyTarget::None
                        || self.instructions.len() != 1
                        || !matches!(
                            context,
                            BodyContext::Unbound | BodyContext::Activated { .. }
                        )
                    {
                        return Err(DefinitionError::InvalidInstruction);
                    }
                    None
                }
                Instruction::ResolvePowerAbility(_) => {
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
                Instruction::BoostSaddledAttacker { .. } => {
                    if !matches!(
                        context,
                        BodyContext::TriggerBound {
                            condition: TriggerSpec {
                                intervening_if: Some(TriggerCondition::SourceSaddled),
                                ..
                            },
                            grouping: TriggerGrouping::EachOccurrence,
                        }
                    ) || self.target != BodyTarget::None
                    {
                        return Err(DefinitionError::InvalidInstruction);
                    }
                    None
                }
                Instruction::CopyEntrantAsArtifact | Instruction::ReplaceSourceCreatedTokens => {
                    if self.target != BodyTarget::None
                        || !matches!(context, BodyContext::TriggerBound {
                        condition,
                        grouping: TriggerGrouping::EachOccurrence,
                    } if *condition == TriggerSpec::enters(ObjectFilter::creature().opposing().nontoken()))
                        || self.instructions.as_slice()
                            != [
                                Instruction::CopyEntrantAsArtifact,
                                Instruction::ReplaceSourceCreatedTokens,
                            ]
                    {
                        return Err(DefinitionError::InvalidInstruction);
                    }
                    None
                }
                Instruction::GiveEquippedCreatureToUpkeepPlayer => {
                    if self.target != BodyTarget::None
                        || !matches!(context, BodyContext::TriggerBound {
                        condition,
                        grouping: TriggerGrouping::EachOccurrence,
                    } if *condition == TriggerSpec::step_began(Step::Upkeep, PlayerRelation::Opponent))
                        || self.instructions.as_slice()
                            != [
                                Instruction::OptionalEffect {
                                    chooser: ResolvingPlayer::Controller,
                                },
                                Instruction::GiveEquippedCreatureToUpkeepPlayer,
                            ]
                    {
                        return Err(DefinitionError::InvalidInstruction);
                    }
                    None
                }
                Instruction::CopyEntrantOnceEachTurn => {
                    if self.target != BodyTarget::None
                        || !matches!(
                            context,
                            BodyContext::TriggerBound {
                                condition: TriggerSpec {
                                    event: TriggerEvent::Enters {
                                        from_zone: TriggerFromZone::Any,
                                        cast_origin: None,
                                        attached_to_creature: false,
                                        ..
                                    },
                                    intervening_if: None,
                                    ..
                                },
                                grouping: TriggerGrouping::EachOccurrence,
                            }
                        )
                        || !matches!(
                            self.instructions.as_slice(),
                            [
                                Instruction::OptionalEffect {
                                    chooser: ResolvingPlayer::Controller
                                },
                                Instruction::CopyEntrantOnceEachTurn
                            ]
                        )
                    {
                        return Err(DefinitionError::InvalidInstruction);
                    }
                    None
                }
                Instruction::ScheduleGraveyardReturnAtEndStep(rule) => {
                    let expected = match rule {
                        DelayedGraveyardReturn::DyingSourceToHand
                        | DelayedGraveyardReturn::DyingSourceTransformed => {
                            TriggerSpec::self_dies()
                        }
                        DelayedGraveyardReturn::DyingCreatureWhileSourceRemains => {
                            TriggerSpec::new(TriggerEvent::Dies {
                                subject: ObjectFilter::creature()
                                    .with_power(PowerFilter::AtMost(1)),
                                mode: DiesTriggerMode::ToControllerGraveyard,
                            })
                        }
                        DelayedGraveyardReturn::AuraAfterEnchantedCreatureDies => {
                            TriggerSpec::dies(ObjectFilter::any().attached_to_self())
                        }
                    };
                    if self.target != BodyTarget::None
                        || !matches!(context, BodyContext::TriggerBound {
                        condition, grouping: TriggerGrouping::EachOccurrence,
                    } if *condition == expected)
                    {
                        return Err(DefinitionError::InvalidInstruction);
                    }
                    None
                }
                Instruction::ReturnSacrificedCardToBattlefield
                    if self.target != BodyTarget::None
                        || !matches!(context, BodyContext::TriggerBound {
                        condition: TriggerSpec {
                            event: TriggerEvent::PlayerAction { action: PlayerTriggerAction::Sacrificed(filter), .. },
                            ..
                        },
                        grouping: TriggerGrouping::EachOccurrence,
                    } if *filter == ObjectFilter::any().nontoken()) =>
                {
                    return Err(DefinitionError::InvalidInstruction);
                }
                Instruction::ReturnSacrificedCardToBattlefield => None,
                Instruction::ReturnDyingCreatureAsForest
                    if self.target == BodyTarget::None
                        && matches!(context,
                    BodyContext::TriggerBound {
                        condition: TriggerSpec {
                            event: TriggerEvent::Dies { subject, mode: DiesTriggerMode::Creature },
                            ..
                        },
                        grouping: TriggerGrouping::EachOccurrence,
                    } if *subject == ObjectFilter::creature().controlled().nontoken().other_than_ability_source()) =>
                {
                    None
                }
                Instruction::ReturnDyingCreatureAsForest => {
                    return Err(DefinitionError::InvalidInstruction);
                }
                Instruction::ReturnDyingSourceTapped
                    if self.target == BodyTarget::None
                        && matches!(context,
                            BodyContext::TriggerBound { condition, grouping: TriggerGrouping::EachOccurrence } if *condition == TriggerSpec::self_dies()
                        ) =>
                {
                    None
                }
                Instruction::ReturnDyingSourceTapped => {
                    return Err(DefinitionError::InvalidInstruction);
                }
                Instruction::PutCountersOnControlledSubtype(kind) => {
                    if vocabulary.label(*kind).is_none() {
                        return Err(DefinitionError::ForeignCreatureType);
                    }
                    None
                }
                Instruction::PutCountersOnControlledCreaturesAndGrant(keywords) => {
                    if self.target != BodyTarget::None
                        || keywords.is_empty()
                        || !matches!(context, BodyContext::TriggerBound { .. })
                    {
                        return Err(DefinitionError::InvalidInstruction);
                    }
                    let parts = ResolvedPartsSpec(vec![
                        crate::characteristics::ResolvedPartSpec::Abilities(
                            crate::characteristics::AbilityChange::grant_keywords(
                                keywords.iter().copied(),
                            ),
                        ),
                    ]);
                    parts
                        .validate(vocabulary, &defined_slots)
                        .map_err(|_| DefinitionError::ContinuousEffect)?;
                    None
                }
                Instruction::PutCountersOnSource { amount } => {
                    let valid_trigger = matches!(
                        context,
                        BodyContext::TriggerBound {
                            condition: TriggerSpec { event, intervening_if: None, zone: TriggerZone::Battlefield },
                            grouping: TriggerGrouping::EachOccurrence,
                        } if match event {
                            TriggerEvent::SpellCast { self_cast: false, .. }
                            | TriggerEvent::Enters { from_zone: TriggerFromZone::Any, cast_origin: None, attached_to_creature: false, .. }
                            | TriggerEvent::PlayerLosesGame(_)
                            | TriggerEvent::Dies { mode: DiesTriggerMode::DamagedBySource, .. }
                            | TriggerEvent::StatusChanged { kind: StatusTriggerKind::Mutated | StatusTriggerKind::Phased { out: false }, .. }
                            | TriggerEvent::PlayerAction { player: PlayerRelation::Controller, action: PlayerTriggerAction::DrewCard { nth_each_turn: Some(_) } } => true,
                            TriggerEvent::PlayerAction { player: PlayerRelation::Controller, action: PlayerTriggerAction::Sacrificed(filter) } => *filter == ObjectFilter::any(),
                            TriggerEvent::Attacks { subject, mode: AttackTriggerMode::Individual { .. } } => *subject == ObjectFilter::self_object(),
                            TriggerEvent::DealsDamage { source, recipient: DamageTriggerRecipient::AnyPlayer, combat_only: true } => *source == ObjectFilter::self_object(),
                            TriggerEvent::CountersChanged { subject, kind: CounterTriggerKind::PlusOnePlaced { .. } } => *subject != ObjectFilter::self_object(),
                            TriggerEvent::Dies { subject: ObjectFilter::Clause(ObjectClause { source_relation: SourceRelation::Any | SourceRelation::OtherThanAbilitySource, .. }), mode: DiesTriggerMode::Creature } => true,
                            TriggerEvent::Dies { subject: ObjectFilter::Clause(c), mode: DiesTriggerMode::Permanent } => c.controller == ControllerFilter::You && !c.any_types.is_empty(),
                            _ => false,
                        }
                    );
                    if (!matches!(
                        context,
                        BodyContext::Unbound
                            | BodyContext::Activated { .. }
                            | BodyContext::SacrificedPermanent
                            | BodyContext::RevealedCreature
                            | BodyContext::ExiledCreatureCost
                            | BodyContext::SacrificedCreature
                    ) && !valid_trigger)
                        || amount < &0.into()
                    {
                        return Err(DefinitionError::InvalidInstruction);
                    }
                    None
                }
                Instruction::Unattach => {
                    let triggered = matches!(context, BodyContext::TriggerBound {
                        condition: TriggerSpec {
                            event: TriggerEvent::DealsDamage {
                                source,
                                recipient: DamageTriggerRecipient::AnyPlayer,
                                combat_only: true,
                            },
                            ..
                        },
                        grouping: TriggerGrouping::EachOccurrence,
                    } if *source == ObjectFilter::any().attached_to_self());
                    let reconfigure = self.instructions.len() == 1
                        && matches!(
                            context,
                            BodyContext::Unbound
                                | BodyContext::Activated { .. }
                                | BodyContext::SacrificedPermanent
                                | BodyContext::RevealedCreature
                                | BodyContext::ExiledCreatureCost
                                | BodyContext::SacrificedCreature
                        );
                    if self.target != BodyTarget::None || !(triggered || reconfigure) {
                        return Err(DefinitionError::InvalidInstruction);
                    }
                    None
                }
                Instruction::Attach(ability) => {
                    let valid = match context {
                        BodyContext::Unbound
                        | BodyContext::Activated { .. }
                        | BodyContext::SacrificedPermanent
                        | BodyContext::RevealedCreature
                        | BodyContext::ExiledCreatureCost
                        | BodyContext::SacrificedCreature
                        | BodyContext::TapCreatureCost => self.target == ability.target(),
                        BodyContext::TriggerBound { .. } => {
                            *ability == crate::attachment::AttachAbility::Equip
                                && self.target
                                    == BodyTarget::FilteredPermanent(
                                        ObjectFilter::creature().controlled(),
                                    )
                        }
                    };
                    if !valid || self.instructions.len() != 1 {
                        return Err(DefinitionError::InvalidInstruction);
                    }
                    Some(OperandFamily::Permanent)
                }
                Instruction::ReturnToOwnersHand
                    if matches!(
                        self.target,
                        BodyTarget::SingleOwnGraveyardCard
                            | BodyTarget::FilteredOwnGraveyardCard(_)
                    ) =>
                {
                    donor_moved = true;
                    Some(OperandFamily::GraveyardCard)
                }
                Instruction::ExileForBlackToken {
                    subtype,
                    size,
                    add_types,
                } => {
                    let expected = if *add_types {
                        BodyTarget::FilteredOwnGraveyardCard(ObjectFilter::enchantment().non_aura())
                    } else {
                        BodyTarget::AnyGraveyardCreature
                    };
                    if self.target != expected
                        || vocabulary.label(*subtype).is_none()
                        || *size == 0
                        || self.instructions.len() != 1
                    {
                        return Err(DefinitionError::InvalidInstruction);
                    }
                    donor_moved = true;
                    Some(OperandFamily::GraveyardCard)
                }
                Instruction::ExileForSubtypeToken(subtype) => {
                    if !matches!(
                        self.target,
                        BodyTarget::AnyGraveyardCreature | BodyTarget::FilteredOwnGraveyardCard(_)
                    ) || vocabulary.label(*subtype).is_none()
                    {
                        return Err(DefinitionError::InvalidInstruction);
                    }
                    Some(OperandFamily::GraveyardCard)
                }
                Instruction::CopyCreatureExiledForAbility => {
                    if !matches!(context, BodyContext::Activated { .. })
                        || self.target != BodyTarget::None
                        || self.instructions.len() != 1
                    {
                        return Err(DefinitionError::InvalidInstruction);
                    }
                    None
                }
                Instruction::DamageFromGrantingPermanent { amount } => {
                    if !amount.is_positive()
                        || self.target != BodyTarget::SingleAnyTarget
                        || self.instructions.len() != 1
                    {
                        return Err(DefinitionError::InvalidInstruction);
                    }
                    Some(OperandFamily::DamageRecipient)
                }
                Instruction::LoseLifeAndScryForSubtype(subtype) => {
                    if self.target != BodyTarget::None || vocabulary.label(*subtype).is_none() {
                        return Err(DefinitionError::InvalidInstruction);
                    }
                    None
                }
                Instruction::ReturnAnyGraveyardCreatureBlackSubtype(kind) => {
                    if self.target != BodyTarget::AnyGraveyardCreature
                        || vocabulary.label(*kind).is_none()
                    {
                        return Err(DefinitionError::OperandFamily);
                    }
                    donor_moved = true;
                    Some(OperandFamily::GraveyardCard)
                }
                Instruction::ReturnAnyGraveyardCreatureOrPlaneswalker => {
                    if self.target != BodyTarget::AnyGraveyardCreatureOrPlaneswalker {
                        return Err(DefinitionError::OperandFamily);
                    }
                    donor_moved = true;
                    Some(OperandFamily::GraveyardCard)
                }
                Instruction::GrantFlashback(FlashbackSubject::Target)
                    if self.target
                        == BodyTarget::FilteredOwnGraveyardCard(
                            ObjectFilter::instant_or_sorcery(),
                        ) =>
                {
                    Some(OperandFamily::GraveyardCard)
                }
                Instruction::GrantFlashback(FlashbackSubject::ControllerGraveyard)
                    if self.target == BodyTarget::None =>
                {
                    None
                }
                Instruction::GrantFlashback(_) => return Err(DefinitionError::OperandFamily),
                Instruction::CastTargetGraveyardInstant => {
                    if self.target != BodyTarget::FilteredOwnGraveyardCard(ObjectFilter::instant())
                    {
                        return Err(DefinitionError::OperandFamily);
                    }
                    Some(OperandFamily::GraveyardCard)
                }
                Instruction::ReturnOwnGraveyardPermanentToBattlefield { .. } => {
                    if !matches!(self.target, BodyTarget::FilteredOwnGraveyardCard(filter) if !filter.allows_instant_or_sorcery())
                    {
                        return Err(DefinitionError::OperandFamily);
                    }
                    donor_moved = true;
                    Some(OperandFamily::GraveyardCard)
                }
                Instruction::ReturnOwnGraveyardCreatureToBattlefield => {
                    if self.target != BodyTarget::FilteredOwnGraveyardCard(ObjectFilter::creature())
                    {
                        return Err(DefinitionError::OperandFamily);
                    }
                    donor_moved = true;
                    Some(OperandFamily::GraveyardCard)
                }
                Instruction::ExileTargetAsAbilityDonor => {
                    if self.target != BodyTarget::SingleCreature
                        || !matches!(context, BodyContext::Activated { .. })
                    {
                        return Err(DefinitionError::InvalidInstruction);
                    }
                    donor_moved = true;
                    Some(OperandFamily::Permanent)
                }
                Instruction::ReturnToOwnersHand
                    if self.target == BodyTarget::OwnedExiledFlashbackCard =>
                {
                    Some(OperandFamily::ExiledCard)
                }
                Instruction::DestroyTarget
                | Instruction::ExileTarget
                | Instruction::ImprintTargetLand
                | Instruction::ImprintTargetCreature
                | Instruction::ExileTargetUntilSourceLeaves
                | Instruction::ExileUntilNextEndStep
                | Instruction::ExileTargetUntilOpponentMonarch
                | Instruction::ReturnToOwnersHand => {
                    if let Some(filter) = match instruction {
                        Instruction::ImprintTargetLand => Some(ObjectFilter::land().controlled()),
                        Instruction::ImprintTargetCreature => {
                            Some(ObjectFilter::creature().nontoken())
                        }
                        _ => None,
                    } && (self.target != BodyTarget::FilteredPermanent(filter)
                        || !matches!(context, BodyContext::TriggerBound {
                            condition, grouping: TriggerGrouping::EachOccurrence,
                        } if *condition == TriggerSpec::self_enters()))
                    {
                        return Err(DefinitionError::InvalidInstruction);
                    }
                    if matches!(instruction, Instruction::DestroyTarget) {
                        facts.checked_destruction_target = true;
                    }
                    if matches!(instruction, Instruction::ExileTarget) && !donor_moved {
                        facts.definitely_exiled_target = true;
                    }
                    donor_moved = true;
                    Some(OperandFamily::Permanent)
                }
            };
            if let Some(family) = family {
                consuming |= reachable;
                let damageable = matches!(family, OperandFamily::DamageRecipient)
                    && matches!(self.target, BodyTarget::FilteredPermanent(filter) if filter.guarantees_damageable());
                if !damageable
                    && !matches!(
                        (self.target, family),
                        (
                            BodyTarget::SingleCreature
                                | BodyTarget::SinglePlaneswalker
                                | BodyTarget::SinglePermanent
                                | BodyTarget::FilteredPermanent(_),
                            OperandFamily::Permanent
                        ) | (
                            BodyTarget::SingleOwnGraveyardCard
                                | BodyTarget::FilteredOwnGraveyardCard(_)
                                | BodyTarget::AnyGraveyardCreatureOrPlaneswalker
                                | BodyTarget::AnyGraveyardCreature
                                | BodyTarget::RecentlyDiscardedOrFromLibrary
                                | BodyTarget::AnyGraveyardCard,
                            OperandFamily::GraveyardCard
                        ) | (
                            BodyTarget::OwnedExiledFlashbackCard,
                            OperandFamily::ExiledCard
                        ) | (
                            BodyTarget::ControlledAbility { .. }
                                | BodyTarget::SingleStackEntry
                                | BodyTarget::StackEntryTargetingControlledPermanent,
                            OperandFamily::StackEntry
                        ) | (BodyTarget::SingleSpell(_), OperandFamily::Spell)
                            | (
                                BodyTarget::SingleOpponent | BodyTarget::SinglePlayer,
                                OperandFamily::Player
                            )
                            | (
                                BodyTarget::SingleCreature
                                    | BodyTarget::SinglePermanent
                                    | BodyTarget::SingleOpponent
                                    | BodyTarget::SinglePlayer
                                    | BodyTarget::SingleCreatureOrPlayer
                                    | BodyTarget::SinglePlayerOrPlaneswalker
                                    | BodyTarget::OpponentOrPlaneswalkerThenCreature
                                    | BodyTarget::SinglePlaneswalker
                                    | BodyTarget::SingleAnyTarget,
                                OperandFamily::DamageRecipient
                            )
                    )
                {
                    return Err(DefinitionError::OperandFamily);
                }
            }
            if reachable {
                facts.definitely = defined_slots;
                facts.donor_moved = donor_moved;
                if let Instruction::ChooseCreatureType { into, .. } = instruction {
                    facts.possibly.insert(*into);
                }
                let next = index
                    .checked_add(1)
                    .ok_or(DefinitionError::InvalidControlFlow)?;
                match instruction {
                    Instruction::Jump { destination } => {
                        facts.merge_into(&mut incoming[*destination])
                    }
                    Instruction::FlipCoin {
                        lose_destination: destination,
                    }
                    | Instruction::BranchIfFalse { destination, .. } => {
                        facts.merge_into(&mut incoming[next]);
                        facts.merge_into(&mut incoming[*destination]);
                    }
                    Instruction::OptionalSequence { instructions, .. } => {
                        let declined = next
                            .checked_add(*instructions)
                            .ok_or(DefinitionError::InvalidControlFlow)?;
                        facts.merge_into(&mut incoming[next]);
                        facts.merge_into(&mut incoming[declined]);
                    }
                    Instruction::OptionalEffect { .. } => {
                        let declined = index
                            .checked_add(2)
                            .ok_or(DefinitionError::InvalidControlFlow)?;
                        facts.merge_into(&mut incoming[next]);
                        facts.merge_into(&mut incoming[declined]);
                    }
                    Instruction::OfferSourceCounters {
                        placed_destination: paid_destination,
                        declined_destination,
                        ..
                    }
                    | Instruction::OfferManaPayment {
                        paid_destination,
                        declined_destination,
                        ..
                    } => {
                        facts.merge_into(&mut incoming[*paid_destination]);
                        facts.merge_into(&mut incoming[*declined_destination]);
                    }
                    Instruction::ReturnCardsDestroyedThisWay
                    | Instruction::Scry { .. }
                    | Instruction::Surveil { .. }
                    | Instruction::Mill { .. }
                    | Instruction::CreateTokenAndAttach { .. }
                    | Instruction::CreateTokensToFight(_)
                    | Instruction::FightPairedCreatures(_)
                    | Instruction::OfferOpponentsDamage
                    | Instruction::TemptWithTokens { .. }
                    | Instruction::CreateTokens { .. }
                    | Instruction::CreateAttackingTokens { .. }
                    | Instruction::PayControllerLife(_)
                    | Instruction::PutEquipmentFromHandOrGraveyard
                    | Instruction::EquipNewToken
                    | Instruction::Encore
                    | Instruction::GrantNewTokensHaste
                    | Instruction::RepeatOpponentLifeLoss
                    | Instruction::OpponentsLoseAndControllerGains(_)
                    | Instruction::AdjustEachOpponentLife { .. }
                    | Instruction::RememberPermanentTarget
                    | Instruction::ControllerLosesRememberedManaValue
                    | Instruction::ExiledTargetControllerGainsPower
                    | Instruction::GoadOtherPlayersCreatures
                    | Instruction::AdditionalCombat { .. }
                    | Instruction::AdditionalCombatAfterUntappingAttackers
                    | Instruction::TakeExtraTurn
                    | Instruction::PreventDamageThisTurn(_)
                    | Instruction::MoveGroup { .. }
                    | Instruction::PutTargetInLibrary(_)
                    | Instruction::PutOwnHandCardOnLibraryBottom
                    | Instruction::PutGroupOnLibraryBottom(_)
                    | Instruction::ExileGraveyardTarget
                    | Instruction::ExileCapturedGraveyardCard
                    | Instruction::GrantGraveyardCastAndWatchAbilities
                    | Instruction::GainCastCardAbilities { .. }
                    | Instruction::ExileCardsFromGraveyard { .. }
                    | Instruction::ReturnUpToOneRecentGraveyardCard
                    | Instruction::ExileUpToTwoGraveyardTargetsGainLife
                    | Instruction::ReturnTargetGraveyardCreatureToHand
                    | Instruction::ReturnRandomGraveyardCards { .. }
                    | Instruction::ChooseRevealedHandCard { .. }
                    | Instruction::ReturnUpToOneOwnGraveyardCreatureToHand
                    | Instruction::ReturnSourceGraveyardToBattlefield { .. }
                    | Instruction::ReturnSourceGraveyardToHand
                    | Instruction::ReturnOwnedCommandersToHand
                    | Instruction::ExileTopForFreePlay
                    | Instruction::ExileTargetUntilNonlandForFreeCast
                    | Instruction::ExileTopRepeatOnPermanentEntry
                    | Instruction::ShuffleEachHandAndGraveyard
                    | Instruction::EndControllerTurn
                    | Instruction::ShuffleOwnLibrary
                    | Instruction::ShuffleOwnGraveyardIntoLibrary
                    | Instruction::ShuffleTargetGraveyardCards { .. }
                    | Instruction::ExileCreatureAtMostRevealedPower
                    | Instruction::CastLinkedSpellMatchingX
                    | Instruction::DestroyDamagedPlayersNonlandsAtX
                    | Instruction::PutChosenXChargeCounters
                    | Instruction::DestroyNonlandsAtSourceChargeCount
                    | Instruction::AddCounterAndDischargeOnResolution(_)
                    | Instruction::ExileCreaturesAndPlaneswalkersBelowRevealedManaValue
                    | Instruction::ExileAllGraveyards
                    | Instruction::DestroyGroupWithoutRegeneration(_)
                    | Instruction::DestroyGroup(_)
                    | Instruction::ExileOnePerPlayerUntilSourceLeaves
                    | Instruction::DoubleCountersOnTargets
                    | Instruction::ExileCombatDamagedCreature
                    | Instruction::DestroyCapturedDamageSource
                    | Instruction::RemoveCreatureCounters { .. }
                    | Instruction::UntapControlledLands
                    | Instruction::UntapControlledCreatures
                    | Instruction::TapAttackersAndSkipNextUntap
                    | Instruction::AttachOnEntry { .. }
                    | Instruction::ControlledSubtypeDamagesAttacker(_)
                    | Instruction::RevealTopAndRemoveAttackerIfForest
                    | Instruction::DamageCapturedBlocker(_)
                    | Instruction::ChangeControlledPlaneswalkers { .. }
                    | Instruction::ChangeAbilitySource { .. }
                    | Instruction::GiveControlOfAbilitySource
                    | Instruction::UntapAndControlCreatures
                    | Instruction::Proliferate
                    | Instruction::AttachControlledToSource { .. }
                    | Instruction::AttachBattlefieldAurasToSource
                    | Instruction::EnterHandOrGraveyardAurasAttachedToSource
                    | Instruction::ReturnDelayedGraveyardToHand(_)
                    | Instruction::LoseLifeAndScryForSubtype(_)
                    | Instruction::ExileForBlackToken { .. }
                    | Instruction::ExileForSubtypeToken(_)
                    | Instruction::GiveEquippedCreatureToUpkeepPlayer
                    | Instruction::CopyCreatureExiledForAbility
                    | Instruction::DamageFromGrantingPermanent { .. }
                    | Instruction::CopyEntrantAsArtifact
                    | Instruction::ReplaceSourceCreatedTokens
                    | Instruction::CopyEntrantOnceEachTurn
                    | Instruction::CopyCreatureAsToken
                    | Instruction::CopyDifferentlyNamedTokens
                    | Instruction::CopyOtherControlledNontokensAsEntryHost
                    | Instruction::DamagePlayersHalfLife
                    | Instruction::DamageGroup { .. }
                    | Instruction::DamageTargetControllersCreatures { .. }
                    | Instruction::DrawAndDamageOpponents { .. }
                    | Instruction::GainControllerLife { .. }
                    | Instruction::DrainTargetAndGain(_)
                    | Instruction::AdjustControllerLife { .. }
                    | Instruction::ExchangeLifeWithSourceToughness
                    | Instruction::AdjustTargetLife { .. }
                    | Instruction::ChooseTypeThenDestroyUnchosen
                    | Instruction::OrderArtifactCreatureLandSacrifices
                    | Instruction::ChooseCreatureTypeForPlayer { .. }
                    | Instruction::ChooseCreatureType { .. }
                    | Instruction::DamageFromPower(_)
                    | Instruction::CreatureDealsPowerDamageToOther { .. }
                    | Instruction::DamageAmount(_)
                    | Instruction::DamageAmountThenExileIfDies(_)
                    | Instruction::DamageCreatureAtSpellMastery { .. }
                    | Instruction::Damage { .. }
                    | Instruction::CreatePreventionShield { .. }
                    | Instruction::CreateReflectingShield
                    | Instruction::CreateDamageMultiplier { .. }
                    | Instruction::ResolveKeywordTrigger(_)
                    | Instruction::IncreaseSpeed
                    | Instruction::BecomeMonarch(_)
                    | Instruction::TapEnchantedCreature
                    | Instruction::RequireCondition(_)
                    | Instruction::RequireTargetAttack
                    | Instruction::RemoveTargetFromCombat
                    | Instruction::RequireAttackSourceNextTurn
                    | Instruction::ChooseAttackDirection
                    | Instruction::ReselectAttackDefender
                    | Instruction::SetBlockDeclarations(_)
                    | Instruction::AdditionalLoyaltyActivation
                    | Instruction::MoveLoyaltyCounterToEnteringPermanent
                    | Instruction::CreateEmblem(_)
                    | Instruction::CreateOpponentEmblems(_)
                    | Instruction::ImpulseCastOrDamage(_)
                    | Instruction::RevealCreatureOfSacrificedType
                    | Instruction::SacrificeCombatContributor
                    | Instruction::ExileNonlandForFormerControllers
                    | Instruction::ExileMilledCreatures { .. }
                    | Instruction::VoteForGraveyardCards { .. }
                    | Instruction::ShuffleOnePermanentPerPlayer
                    | Instruction::ReturnExiledTargetCohort { .. }
                    | Instruction::RevealCreatureForEachTargetController
                    | Instruction::ReturnOddGraveyardPermanentIfPaid { .. }
                    | Instruction::PutGraveyardCreatureAtMostX
                    | Instruction::PutLandFromExiledCohort
                    | Instruction::ExileTopForEachSpellType(_)
                    | Instruction::ExileTopFreeNoncreatureSpells(_)
                    | Instruction::ExileRelatedTopForCastThisTurn
                    | Instruction::ExileRelatedTopForPlay
                    | Instruction::ExileTopForPlay { .. }
                    | Instruction::ExileTopFaceDown
                    | Instruction::ScheduleCreatedTokenExile(_)
                    | Instruction::ScheduleCreatedTokenSacrifice
                    | Instruction::ExileDelayedObjects(_)
                    | Instruction::ExileRelatedPermanents(_)
                    | Instruction::ExileTargetWithLink(_)
                    | Instruction::ReturnCardsExiledWithLink(_)
                    | Instruction::ChooseAndReturnOwnGraveyardCardsByType
                    | Instruction::ReturnOwnGraveyardCardsOfTypeToHand(_)
                    | Instruction::TargetDamagesItsControllersOtherCreatures
                    | Instruction::OtherCreaturesReturnPowerDamage
                    | Instruction::DestroyCapturedCombatCreature { .. }
                    | Instruction::ReflectAttachedCreatureDamage
                    | Instruction::DestroyAttachedEnchantmentSubtype(_)
                    | Instruction::GrantFlashback(_)
                    | Instruction::TapTargetWhileControllingSource
                    | Instruction::RegisterTokenLinkedExile
                    | Instruction::ReturnTokenLinkedExile
                    | Instruction::AddMana(_)
                    | Instruction::AbandonSourceScheme
                    | Instruction::TabletopHonorAction { .. }
                    | Instruction::CreateNonlegendaryLoyaltyCopy
                    | Instruction::WeakenOpposingAttackersUntilNextTurn
                    | Instruction::ChooseMeldPartner(_)
                    | Instruction::ExileMeldPair
                    | Instruction::MeldExiledPair(_)
                    | Instruction::PairWithCreature { .. }
                    | Instruction::PhaseOtherPlaneswalkersNextEndStep
                    | Instruction::PhaseOut(_)
                    | Instruction::PhaseOutObjects(_)
                    | Instruction::ReturnOneNonlandPerOpponent
                    | Instruction::WeakenDelayedAttacker(_)
                    | Instruction::DrawRevealRemoveLoyalty
                    | Instruction::CreateReflexiveDiscardDamage
                    | Instruction::RemoveLoyaltyExileTopForPlay
                    | Instruction::DamageDamagedPlayersCreatures
                    | Instruction::DamageOpponentsAmount(_)
                    | Instruction::DamageRelatedPlayerAmount(_)
                    | Instruction::Ninjutsu
                    | Instruction::Unearth
                    | Instruction::Cascade
                    | Instruction::CascadeBottom
                    | Instruction::Myriad
                    | Instruction::CopyForOtherCreatures
                    | Instruction::CopyResolvingSpell
                    | Instruction::WatchNextSpellOrLoyaltyTwice
                    | Instruction::WatchNextLoyaltyCopy
                    | Instruction::ChoosePlaneswalkerTypeForCopies
                    | Instruction::CopyCapturedAbility
                    | Instruction::CopyOnCast(_)
                    | Instruction::MadnessCast
                    | Instruction::MiracleCast(_)
                    | Instruction::CastDefeatedSiege
                    | Instruction::CastTargetGraveyardInstant
                    | Instruction::ReboundCast(_)
                    | Instruction::EndStepCast { .. }
                    | Instruction::SacrificeDelayedObjects(_)
                    | Instruction::ScheduleArtifactReturn(_)
                    | Instruction::ScheduleGraveyardReturnAtEndStep(_)
                    | Instruction::ReturnSacrificedCardToBattlefield
                    | Instruction::ReturnDyingCreatureAsForest
                    | Instruction::ReturnDyingSourceTapped
                    | Instruction::ReturnDelayedGraveyardCard(_)
                    | Instruction::ReturnDelayedGraveyardCardTransformed(_)
                    | Instruction::ChooseFoundSpell
                    | Instruction::CastFoundSpell(_)
                    | Instruction::ReturnDelayedPermanentToHand(_)
                    | Instruction::ReturnDelayedExileWithCounters(_)
                    | Instruction::ReturnDelayedExile(_)
                    | Instruction::ExileUnearthed(_)
                    | Instruction::DrainOpponents(_)
                    | Instruction::ExtortDrain
                    | Instruction::ResolveTransfer(_)
                    | Instruction::ResolveGrowth(_)
                    | Instruction::ResolveCounterKeyword(_)
                    | Instruction::SacrificeEvoked
                    | Instruction::SacrificeStateTriggerSource
                    | Instruction::SacrificeUpkeepSource(_)
                    | Instruction::CounterWardSource
                    | Instruction::BoostSourceByCastManaValue
                    | Instruction::BoostLoneAttacker
                    | Instruction::SetControlledCreatureSizeToHand
                    | Instruction::ModifyGroupByAmount { .. }
                    | Instruction::AddExperienceCounter
                    | Instruction::ControlManaSourceThroughNextTurn
                    | Instruction::GrantControllerFlashUntilCleanup
                    | Instruction::RequireOpponentsCreaturesBlockThisTurn
                    | Instruction::RequireBlocksAgainstTargetThisTurn
                    | Instruction::SuppressGroupKeywords { .. }
                    | Instruction::ModifyGroupUntilEndOfTurn { .. }
                    | Instruction::BoostSource { .. }
                    | Instruction::ResolvePowerAbility(_)
                    | Instruction::BoostSaddledAttacker { .. }
                    | Instruction::Adapt(_)
                    | Instruction::TransformSource
                    | Instruction::TransformSourceAndAttachToTarget
                    | Instruction::TransformSubtype(_)
                    | Instruction::ClearSourceCounters(_)
                    | Instruction::PutCountersOnSource { .. }
                    | Instruction::UnattachEquipmentAndProtectHost
                    | Instruction::Unattach
                    | Instruction::Attach(_)
                    | Instruction::PutCounterOnRelatedPermanent
                    | Instruction::PutCountersOnControlledCreatures
                    | Instruction::PutCountersOnControlledSubtype(_)
                    | Instruction::PutCountersOnControlledCreaturesAndGrant(_)
                    | Instruction::PutCountersOnOtherCreatures
                    | Instruction::PutCreatureAndOtherPlaneswalkerCounters
                    | Instruction::PutChargeCounterOnSource
                    | Instruction::PutBlessingCounterOnSource
                    | Instruction::PutNamedCounterOnSource { .. }
                    | Instruction::PutQuestCounterOnSource
                    | Instruction::PutStorageCounterOnSource
                    | Instruction::AbilitySourceControllerLosesLife(_)
                    | Instruction::RelatedPlayerLosesLife(_)
                    | Instruction::LinkedPlayerLosesDamageAmount
                    | Instruction::RelatedPlayerLosesHalfLife
                    | Instruction::RegenerateTarget
                    | Instruction::ExileDefeatedSiege
                    | Instruction::ExileSourceAndGainLife(_)
                    | Instruction::ExileAbilitySource
                    | Instruction::ReturnSourceToHand
                    | Instruction::UntapAbilitySource
                    | Instruction::TapAbilitySource
                    | Instruction::RegenerateSource
                    | Instruction::RegenerateControlledSubtype(_)
                    | Instruction::GoadTarget
                    | Instruction::DestroyTarget
                    | Instruction::ExileTarget
                    | Instruction::ImprintTargetLand
                    | Instruction::ImprintTargetCreature
                    | Instruction::ExileTargetAsAbilityDonor
                    | Instruction::ExileTargetUntilSourceLeaves
                    | Instruction::ExileUntilNextEndStep
                    | Instruction::ExileTargetUntilOpponentMonarch
                    | Instruction::ReturnToOwnersHand
                    | Instruction::ReturnOwnGraveyardCreatureToBattlefield
                    | Instruction::ReturnOwnGraveyardPermanentToBattlefield { .. }
                    | Instruction::ReturnAnyGraveyardCreatureBlackSubtype(_)
                    | Instruction::ReturnAnyGraveyardCreatureOrPlaneswalker
                    | Instruction::CounterSpell
                    | Instruction::CounterSpellToExile
                    | Instruction::ChooseNewTargets
                    | Instruction::GainControlOfSpell
                    | Instruction::ChangeOnlyTarget
                    | Instruction::LoseLifeEqualToTargetSpellManaValue
                    | Instruction::CopyAbility
                    | Instruction::CopySpell
                    | Instruction::CopyTargetToSource { .. }
                    | Instruction::CreateContinuousEffect { .. }
                    | Instruction::WeakenTargetByChosenX
                    | Instruction::BoostTargetPower { .. }
                    | Instruction::AnimateTargetAsCountedCreature { .. }
                    | Instruction::WatchSmallCreatureCombatDraw(_)
                    | Instruction::WatchTargetCombatDamageDraw
                    | Instruction::GrantProtectionChosenColor(_)
                    | Instruction::PutIndestructibleCounter
                    | Instruction::PutLoyaltyCounter
                    | Instruction::PutPowerToughnessCounters { .. }
                    | Instruction::FixedTargetClause(_)
                    | Instruction::ExileResolvingSpell
                    | Instruction::DestroyFixedTargets
                    | Instruction::DestroyTargetAndAttachedEquipment
                    | Instruction::ReturnFixedTargets
                    | Instruction::FightSourceWithTarget
                    | Instruction::FightFixedTargets
                    | Instruction::ExileUntilOtherTargetLeaves
                    | Instruction::CopyOtherTargetAsArtifact
                    | Instruction::ReturnChosenXGraveyardCreatures
                    | Instruction::ReturnGraveyardTargetsTapped(_)
                    | Instruction::ReturnCapturedDyingCreature
                    | Instruction::ReturnCapturedDyingCreatureWithTypeAndColor { .. }
                    | Instruction::ReturnGraveyardCreatureWithSubtypeCounters { .. }
                    | Instruction::ReturnGraveyardCohort(_)
                    | Instruction::DivideThreeDamageAmongTargets
                    | Instruction::DistributePlusOneCounters { .. }
                    | Instruction::DamageOrderedTargets { .. }
                    | Instruction::BindOrderedPermanents { .. }
                    | Instruction::ExileOrderedPermanent(_)
                    | Instruction::ExileLibraryTop { .. }
                    | Instruction::PutTargetOwnedExiledLandsOntoBattlefield
                    | Instruction::LookOpponentExileForCast { .. }
                    | Instruction::ExileFromTargetLibrary
                    | Instruction::OfferVariablePayment { .. }
                    | Instruction::DrawEachPlayerForPayments
                    | Instruction::CreateReflexiveGraveyardReturn
                    | Instruction::ReturnGraveyardCreatureWithCorpse(_)
                    | Instruction::ReturnRandomLinkedExile
                    | Instruction::ReturnLinkedSearchCards
                    | Instruction::PutHandPermanentsOntoBattlefield { .. }
                    | Instruction::MayTurnControlledCreatureFaceUp
                    | Instruction::ExploreSource
                    | Instruction::MayRevealLibraryTop
                    | Instruction::ManifestTop { .. }
                    | Instruction::ExileNonlandsToHandAndDamage { .. }
                    | Instruction::RevealTopHandLoseManaValue { .. }
                    | Instruction::RevealSharedTypeCreatureCastOrBottom
                    | Instruction::RevealOddCastOrDraw
                    | Instruction::RevealUntilLandKind
                    | Instruction::RevealOpponentChoosesCreature
                    | Instruction::ShuffleAbilitySourceIntoLibrary
                    | Instruction::RevealToNameAfterSourceShuffle(_)
                    | Instruction::ChooseOwnCommanderToCast
                    | Instruction::ChooseMatchingHandSpellToCast
                    | Instruction::SearchEachLibraryForFreeCast
                    | Instruction::LookTopOneRestGraveyard { .. }
                    | Instruction::RevealTopHandRestGraveyard { .. }
                    | Instruction::LookTopOneRestBottom { .. }
                    | Instruction::LookTopCreatureSharingAttachedSubtype { .. }
                    | Instruction::RevealTopCreaturesAndLandsRestBottom { .. }
                    | Instruction::RevealHandAndBottomPiles
                    | Instruction::RevealDividePiles
                    | Instruction::DivideTargetPermanentsAndSacrificePile
                    | Instruction::DrawToChosenOpponentHandSize
                    | Instruction::LookTopPlaneswalkers
                    | Instruction::RevealTopPermanentOrHand
                    | Instruction::LookTopLandBattlefieldTapped
                    | Instruction::LookTopReveal { .. }
                    | Instruction::LookTopTakeOne { .. }
                    | Instruction::SearchLibrary { .. }
                    | Instruction::DrawAndLoseLifeForTargetGraveyardCreatures
                    | Instruction::DrawControllerAndTarget { .. }
                    | Instruction::DrawTargetPlayer { .. }
                    | Instruction::DrawEachPlayer { .. }
                    | Instruction::DrawHighestEntrantController
                    | Instruction::DrawRelatedPlayer
                    | Instruction::DrawCombatDamageSourceController
                    | Instruction::DrawPlayersControllingName(_)
                    | Instruction::DrawController { .. }
                    | Instruction::ModifyAnyNumberOfPermanents { .. }
                    | Instruction::AffectPermanentTargets { .. }
                    | Instruction::MillEachPlayer(_)
                    | Instruction::PutLibraryCohortIntoGraveyard(_)
                    | Instruction::Bolster(_)
                    | Instruction::CreateArmyIfAbsent { .. }
                    | Instruction::Amass { .. }
                    | Instruction::UntapUpTo { .. }
                    | Instruction::ChooseVowedCreatures
                    | Instruction::ChooseCreaturesToKeep
                    | Instruction::DestroyOtherCreatures
                    | Instruction::SacrificeOtherCreatures
                    | Instruction::SacrificeColoredPermanents
                    | Instruction::ExileChosenPermanents { .. }
                    | Instruction::ExileFromHandOrBattlefield(_)
                    | Instruction::ExileRelatedHandCardForPlay
                    | Instruction::PayLifeOrSacrificeCreatures
                    | Instruction::ReturnControlledPermanents { .. }
                    | Instruction::ChooseOtherControlledCreature { .. }
                    | Instruction::Sacrifice { .. }
                    | Instruction::DiscardOneAtRandom(_)
                    | Instruction::SacrificeRandomUntilLand
                    | Instruction::DiscardUpTo { .. }
                    | Instruction::SacrificeToContinue { .. }
                    | Instruction::SacrificeMatching(_)
                    | Instruction::Discard { .. } => facts.merge_into(&mut incoming[next]),
                }
            }
        }
        if self.target != BodyTarget::None && !consuming {
            return Err(DefinitionError::PhantomTarget);
        }
        Ok(())
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
    pub allow_repeat: bool,
    pub additional: Vec<ExecutableBody>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpellDefinition {
    /// On a modal spell, `body` is the first printed mode and this field contains the rest.
    pub modes: Option<SpellModes>,
    pub cost: ManaCost,
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
                vec![Instruction::ResolveCounterKeyword(self)],
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
                    Instruction::OfferManaPayment {
                        payer: ResolvingPlayer::Controller,
                        cost,
                        paid_destination: 2,
                        declined_destination: 1,
                    },
                    Instruction::SacrificeUpkeepSource(self),
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
                    Instruction::OfferManaPayment {
                        payer: ResolvingPlayer::UniqueRelatedPlayer,
                        cost,
                        paid_destination: 2,
                        declined_destination: 1,
                    },
                    Instruction::CounterWardSource,
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
                vec![Instruction::Cascade, Instruction::CascadeBottom],
            ),
        }
    }
    /// Storm, gravestorm and the triggered half of a printed replicate cost.
    pub fn spell_copies(id: TriggeredAbilityId, kind: CastCopyKeyword) -> Self {
        Self {
            id,
            condition: TriggerSpec::self_cast(),
            grouping: TriggerGrouping::EachOccurrence,
            body: ExecutableBody::new(BodyTarget::None, vec![Instruction::CopyOnCast(kind)]),
        }
    }
    pub fn cast_copy_keyword(&self) -> Option<CastCopyKeyword> {
        let [Instruction::CopyOnCast(kind)] = self.body.instructions.as_slice() else {
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
                vec![Instruction::Sacrifice {
                    players: AffectedPlayers::One(ResolvingPlayer::UniqueRelatedPlayer),
                    amount: ValueExpr::Literal(amount),
                    filter: PermanentTypeFilter {
                        any_types: Default::default(),
                        required_types: BTreeSet::new(),
                        forbidden_types: BTreeSet::new(),
                    },
                }],
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
                    Instruction::OfferManaPayment {
                        payer: ResolvingPlayer::Controller,
                        cost: ManaCost::generic(1.into()),
                        paid_destination: 1,
                        declined_destination: 2,
                    },
                    Instruction::ExtortDrain,
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
                vec![Instruction::ResolveKeywordTrigger(keyword)],
            ),
        }
    }

    /// Identify a printed keyword only when the entire executable definition matches it.
    pub fn as_combat_keyword(&self) -> Option<&CombatTriggerKeyword> {
        let [Instruction::ResolveKeywordTrigger(keyword)] = self.body.instructions.as_slice()
        else {
            return None;
        };
        (self.condition == keyword.condition()
            && self.grouping == TriggerGrouping::EachOccurrence
            && self.body.target == BodyTarget::None)
            .then_some(keyword)
    }

    /// Whether this definition has the exact rules text of exalted.
    pub fn is_exalted(&self) -> bool {
        self.condition == TriggerSpec::attacks(ObjectFilter::any(), AttackTriggerMode::Alone)
            && self.grouping == TriggerGrouping::EachOccurrence
            && self.body.target == BodyTarget::None
            && self.body.instructions == [Instruction::BoostLoneAttacker]
    }
    /// Construct one independent instance of exalted (CR 702.83).
    pub fn exalted(id: TriggeredAbilityId) -> Self {
        Self {
            id,
            condition: TriggerSpec::attacks(ObjectFilter::any(), AttackTriggerMode::Alone),
            grouping: TriggerGrouping::EachOccurrence,
            body: ExecutableBody::new(BodyTarget::None, vec![Instruction::BoostLoneAttacker]),
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
}
/// Artifact subtypes represented in the printed and evaluated characteristics.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ArtifactSubtype {
    Book,
    Clue,
    Treasure,
    Food,
    Blood,
    Powerstone,
    Gold,
    Equipment,
}
impl ArtifactSubtype {
    /// Exact printed subtype word.
    pub const fn wire_label(self) -> &'static str {
        match self {
            Self::Book => "Book",
            Self::Clue => "Clue",
            Self::Treasure => "Treasure",
            Self::Food => "Food",
            Self::Blood => "Blood",
            Self::Powerstone => "Powerstone",
            Self::Gold => "Gold",
            Self::Equipment => "Equipment",
        }
    }
    /// Resolve an exact supported subtype.
    pub fn from_wire_label(label: &str) -> Option<Self> {
        [
            Self::Book,
            Self::Clue,
            Self::Treasure,
            Self::Food,
            Self::Blood,
            Self::Powerstone,
            Self::Gold,
            Self::Equipment,
        ]
        .into_iter()
        .find(|kind| kind.wire_label() == label)
    }
}
/// Enchantment subtype supported by the current characteristic model.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum EnchantmentSubtype {
    Curse,
    /// Saga (CR 205.3h).
    Saga,
}
impl EnchantmentSubtype {
    /// Stable subtype label for authored definitions.
    pub const fn wire_label(self) -> &'static str {
        match self {
            Self::Saga => "Saga",
            Self::Curse => "Curse",
        }
    }
    /// Parse the supported exact printed subtype label.
    pub fn from_wire_label(label: &str) -> Option<Self> {
        match label {
            "Saga" => Some(Self::Saga),
            "Curse" => Some(Self::Curse),
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
        }
    }
    /// Parse an exact supported subtype label.
    pub fn from_wire_label(label: &str) -> Option<Self> {
        match label {
            "Adventure" => Some(Self::Adventure),
            "Arcane" => Some(Self::Arcane),
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
/// Authored partial definition. Only validated definitions enter EffectWindow.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Program {
    pub splice: Option<SpliceAbility>,
    /// Whether this spell has a printed ability preventing it from being countered.
    pub cannot_be_countered: bool,
    /// Number of {X} symbols in the printed mana cost, each sharing the announced value.
    pub x_mana_symbols: u8,
    /// Hybrid and Phyrexian symbols in printed order. Their first colors are in the base price.
    pub hybrid_symbols: Vec<crate::mana::HybridSymbol>,
    pub attack_requirement: Option<AttackRequirement>,
    pub entry_copy: Option<EntryCopyRule>,
    /// Limits contributed while these printed abilities are active.
    pub combat_limits: CombatDeclarationLimits,
    /// This spell can be cast only during combat on its controller’s turn.
    pub cast_during_own_combat: bool,
    /// Mandatory life payment on casting. Fixed and announced X prices are admitted.
    pub additional_life_cost: Option<ValueExpr>,
    /// Physical reverse face of a transforming double-faced card.
    pub nightbound_back: Option<Box<Program>>,
    /// Physical reverse face of a transforming card without daybound.
    pub transform_back: Option<Box<Program>>,
    /// Independent alternative characteristics used when casting an Adventure.
    pub adventure: Option<Box<Program>>,
    /// Printed halves of a split instant or sorcery, combined outside the stack.
    pub split: Option<Box<SplitCard>>,
    /// Printed or copied keyword abilities before continuous effects are applied.
    pub printed_keywords: BTreeSet<KeywordAbility>,
    /// Printed permissions for face-down casting and special-action costs.
    pub face_up_abilities: Vec<crate::face_down::FaceUpAbility>,
    /// Restrictions keyed by the printed activated ability instance.
    pub activation_restrictions:
        BTreeMap<crate::activated::ActivatedAbilityId, crate::activated::ActivationRestriction>,
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
    /// Creature cards the caster must exile from their graveyard to cast this spell.
    pub graveyard_creature_exile_cost: u8,
    /// Exile X cards from the casting player's graveyard as an additional cost.
    pub graveyard_exile_x_cost: bool,
    /// Printed permission for a creature to be cast from its owner's graveyard.
    pub graveyard_self_cast: Option<GraveyardSelfCast>,
    /// Casting this spell requires sacrificing one creature the caster controls.
    pub sacrifice_creature_cast_cost: bool,
    /// The caster may tap any number of untapped creatures they control as an additional cost.
    pub tap_creatures_cast_cost: bool,
    /// Reveal a colorless creature card in hand as an additional casting cost.
    pub reveal_colorless_creature_cast_cost: bool,
    /// Printed alternative price which depends on an event earlier this turn.
    pub conditional_cost: Option<ConditionalCastCost>,
    /// Optional mana costs on an instruction spell.
    pub additional_cast_costs: Vec<AdditionalCastCost>,
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
    /// TargetSpellController requires SingleSpell; TargetPlayer requires SinglePlayer.
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
            && matches!(
                spell.body.instructions.as_slice(),
                [Instruction::Damage { .. }
                    | Instruction::DamageAmount(_)
                    | Instruction::DrawController { .. }
                    | Instruction::GainControllerLife { .. }
                    | Instruction::DrainTargetAndGain(_)
                    | Instruction::AdjustControllerLife { .. }
                    | Instruction::ReturnToOwnersHand
                    | Instruction::DestroyTarget
                    | Instruction::AddMana(EffectManaYield::Fixed(_))
                    | Instruction::CreateTokens { .. }
                    | Instruction::CreateAttackingTokens { .. }
                    | Instruction::ModifyGroupUntilEndOfTurn { .. }
                    | Instruction::CreateContinuousEffect { .. }
                    | Instruction::GrantProtectionChosenColor(_)
                    | Instruction::Mill { .. }]
            )
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
    /// Printed mana cost of this face, including the distinction between an absent and a zero cost.
    pub fn printed_mana_cost(&self) -> Option<&ManaCost> {
        match &self.kind {
            ProgramKind::Spell(spell) => Some(&spell.cost),
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
            card_types,
            creature_subtypes,
            land_subtypes,
            spell_subtypes: BTreeSet::new(),
            artifact_subtypes: BTreeSet::new(),
            enchantment_subtypes: BTreeSet::new(),
            planeswalker_subtypes: BTreeSet::new(),
            splice: None,
            adventure: None,
            split: None,
            nightbound_back: None,
            transform_back: None,
            printed_keywords: BTreeSet::new(),
            face_up_abilities: Vec::new(),
            activation_restrictions: BTreeMap::new(),
            loyalty_costs: BTreeMap::new(),
            variable_loyalty_costs: BTreeSet::new(),
            printed_toxic: None,
            x_mana_symbols: 0,
            hybrid_symbols: Vec::new(),
            attack_requirement: None,
            entry_copy: None,
            combat_limits: Default::default(),
            cast_during_own_combat: false,
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
            tap_creatures_cast_cost: false,
            reveal_colorless_creature_cast_cost: false,
            discard_cast_cost: false,
            graveyard_creature_exile_cost: 0,
            graveyard_exile_x_cost: false,
            graveyard_self_cast: None,
            additional_cast_costs: vec![],
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
                    vec![Instruction::DrawController {
                        amount: ValueExpr::Literal(1.into()),
                    }],
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
            body: ExecutableBody::new(BodyTarget::None, vec![Instruction::SacrificeEvoked]),
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
                body: ExecutableBody::new(attach.target(), vec![Instruction::Attach(attach)]),
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
                    BodyTarget::SingleCreature,
                    vec![Instruction::Attach(AttachAbility::Reconfigure)],
                ),
            },
            ActivatedAbility {
                id: ActivatedAbilityId(2),
                mana_cost: cost,
                source_cost: SourceCost::None,
                body: ExecutableBody::new(BodyTarget::None, vec![Instruction::Unattach]),
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
        if !self.hybrid_symbols.is_empty() {
            let mut required = self
                .printed_mana_cost()
                .ok_or(DefinitionError::InvalidInstruction)?
                .required
                .clone();
            for symbol in &self.hybrid_symbols {
                if Color::from_mana(symbol.first).is_none()
                    || matches!(symbol.alternative,
                    crate::mana::HybridAlternative::Mana(other) if symbol.first == other || Color::from_mana(other).is_none())
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
            for half in [&split.left, &split.right] {
                if half.split.is_some() || half.adventure.is_some() || half.permanent().is_some() {
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
                CastCopyKeyword::Replicate(_) => self.has_type(CardType::Land),
                _ => !matches!(self.kind, ProgramKind::Spell(_)),
            }) || matches!(copy,CastCopyKeyword::Replicate(i) if !matches!(self.additional_cast_costs.get(i),Some(AdditionalCastCost::Replicate(_))))
            {
                return Err(DefinitionError::TriggeredSource);
            }
        }
        zone_replacement::validate(self)?;
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
                || !price.is_chosen_x()
                    && !matches!(price, ValueExpr::Literal(n) if n >= &0.into()))
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
                                | AdditionalCastCost::Multikicker(_)
                                | AdditionalCastCost::Replicate(_)
                        )
                }
            };
            if !valid_kind || !additional.mana().valid() {
                return Err(DefinitionError::CastingCost);
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
        if let Some(permission) = self.graveyard_self_cast
            && (!self.has_type(CardType::Creature)
                || self.printed_mana_cost().is_none()
                || matches!(permission, GraveyardSelfCast::WhileControllingSubtype(kind) if vocabulary.label(kind).is_none()))
        {
            return Err(DefinitionError::CastingCost);
        }
        if self.graveyard_exile_x_cost && !matches!(self.kind, ProgramKind::Spell(_)) {
            return Err(DefinitionError::InvalidInstruction);
        }
        if self.graveyard_creature_exile_cost > 0 && self.printed_mana_cost().is_none() {
            return Err(DefinitionError::CastingCost);
        }
        if self.discard_cast_cost && !matches!(self.kind, ProgramKind::Spell(_)) {
            return Err(DefinitionError::CastingCost);
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
            if !matches!(self.additional_cast_costs.get(*index),Some(AdditionalCastCost::KickerX(_) | AdditionalCastCost::Kicker(_) | AdditionalCastCost::Multikicker(_))))) {return Err(DefinitionError::InvalidInstruction);}
        if self.entry_counter_abilities.iter().any(|ability| {
            matches!(ability,
                EntryCounterAbility::ControlledAndGraveyardSubtypePlusOne(subtype)
                | EntryCounterAbility::GraveyardControlledSubtypePlusOne(subtype)
                    if vocabulary.label(*subtype).is_none()
            )
        }) {
            return Err(DefinitionError::InvalidInstruction);
        }
        if !self.entry_counter_abilities.is_empty() && (!matches!(self.kind,ProgramKind::Permanent(_)) || self.entry_counter_abilities.iter().any(|ability|matches!(ability, EntryCounterAbility::Bloodthirst(n) | EntryCounterAbility::FixedCharge(n) | EntryCounterAbility::FixedLoyalty(n) | EntryCounterAbility::FixedPlusOne(n) | EntryCounterAbility::MorbidPlusOne(n) if n < &0.into()))) { return Err(DefinitionError::InvalidInstruction); }
        for reduction in &self.cast_reductions {
            if matches!(
                reduction,
                CastReduction::ExileGraveyardCreatures { per_card: 0 }
            ) {
                return Err(DefinitionError::CastingCost);
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
            Some(ConditionalCastCost::Evoke(_))
                if !permanent
                    || !self.triggered.iter().any(|a| {
                        matches!(
                            a.condition.event,
                            TriggerEvent::Enters {
                                cast_origin: Some(CastEntryKind::Evoked),
                                ..
                            }
                        ) && a.body.instructions.as_slice() == [Instruction::SacrificeEvoked]
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
                                == [Instruction::DrawController {
                                    amount: ValueExpr::Literal(1.into()),
                                }]
                        })) =>
            {
                return Err(DefinitionError::CastingCost);
            }
            _ => {}
        }
        if self.entry_copy.is_some() && self.permanent().is_none() {
            return Err(DefinitionError::InvalidInstruction);
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
                if value
                    .casting_cost
                    .as_ref()
                    .is_some_and(|cost| !cost.valid())
                    || self.has_type(CardType::Land) && value.casting_cost.is_some()
                {
                    return Err(DefinitionError::CastingCost);
                }
                if matches!(
                    value.entry_tap,
                    Some(crate::land::EntryTap::AlwaysChooseColorExcept(
                        crate::mana::ManaType::Colourless
                    ))
                ) || self.mana.iter().any(|ability| {
                    ability
                        .output
                        .entry_chosen_fixed_color()
                        .is_some_and(|fixed| {
                            value.entry_tap
                                != Some(crate::land::EntryTap::AlwaysChooseColorExcept(fixed))
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
                        Instruction::Encore
                        | Instruction::CastLinkedSpellMatchingX
                        | Instruction::DestroyDamagedPlayersNonlandsAtX
                        | Instruction::PutChosenXChargeCounters
                        | Instruction::DestroyNonlandsAtSourceChargeCount
                        | Instruction::AddCounterAndDischargeOnResolution(_) => true,
                        Instruction::LookTopReveal {
                            filter:
                                LookTopFilter::SourceChosenCreatureType
                                | LookTopFilter::SourceChosenCreatureTypeKeepRest,
                            ..
                        }
                        | Instruction::MayTurnControlledCreatureFaceUp
                        | Instruction::ExploreSource
                        | Instruction::MayRevealLibraryTop
                        | Instruction::ChangeAbilitySource { .. }
                        | Instruction::GiveControlOfAbilitySource
                        | Instruction::PreventDamageThisTurn(
                            crate::damage::TurnPreventionFilter::ToAbilitySource,
                        )
                        | Instruction::RequireAttackSourceNextTurn
                        | Instruction::ExileUntilNextEndStep
                        | Instruction::AttachControlledToSource { .. }
                        | Instruction::UntapAbilitySource
                        | Instruction::TapAbilitySource
                        | Instruction::RegenerateSource
                        | Instruction::ExileDefeatedSiege
                        | Instruction::ExileSourceAndGainLife(_)
                        | Instruction::ExileAbilitySource
                        | Instruction::ReturnSourceToHand
                        | Instruction::ImprintTargetLand
                        | Instruction::ImprintTargetCreature
                        | Instruction::ExileTargetAsAbilityDonor
                        | Instruction::ExileTargetUntilSourceLeaves
                        | Instruction::PutNamedCounterOnSource { .. }
                        | Instruction::PutChargeCounterOnSource
                        | Instruction::PutBlessingCounterOnSource
                        | Instruction::PutQuestCounterOnSource
                        | Instruction::PutStorageCounterOnSource
                        | Instruction::PutCounterOnRelatedPermanent
                        | Instruction::PutCountersOnControlledCreatures
                        | Instruction::PutCountersOnControlledSubtype(_)
                        | Instruction::PutCountersOnControlledCreaturesAndGrant(_)
                        | Instruction::PutCountersOnOtherCreatures
                        | Instruction::DamageFromPower(PowerDamageSource::AbilitySource) => true,
                        Instruction::ModifyGroupByAmount { group, .. }
                        | Instruction::ModifyGroupUntilEndOfTurn { group, .. }
                            if matches!(group.as_ref(), EffectSubject::AbilitySource) =>
                        {
                            true
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
                        Instruction::DivideThreeDamageAmongTargets
                            | Instruction::DamageOrderedTargets { .. }
                            | Instruction::DamageFromGrantingPermanent { .. }
                            | Instruction::CreateReflexiveDiscardDamage
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
                    && (spell.modes.is_some()
                        || spell.body.target != BodyTarget::None
                        || spell.body.permanent_target_bounds().is_some())
                {
                    return Err(DefinitionError::InvalidInstruction);
                }
                if spell
                    .body
                    .instructions
                    .iter()
                    .enumerate()
                    .any(|(index, op)| {
                        matches!(op, Instruction::ExileResolvingSpell)
                            && index + 1 != spell.body.instructions.len()
                    })
                {
                    return Err(DefinitionError::InvalidInstruction);
                }
                let cost_context = if self.reveal_colorless_creature_cast_cost {
                    BodyContext::RevealedCreature
                } else if self.graveyard_creature_exile_cost == 1 {
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
                        .any(|body| {
                            body.dependent_targets().is_some()
                                || body.instructions.iter().any(|op| {
                                    matches!(
                                        op,
                                        Instruction::FixedTargetClause(_)
                                            | Instruction::BindOrderedPermanents { .. }
                                            | Instruction::ExileResolvingSpell
                                    )
                                })
                        })
                    {
                        return Err(DefinitionError::InvalidInstruction);
                    }
                    if modes.minimum == 0
                        || modes.maximum < modes.minimum
                        || modes.additional.is_empty()
                        || (!modes.allow_repeat
                            && usize::from(modes.maximum) > modes.additional.len() + 1)
                    {
                        return Err(DefinitionError::InvalidInstruction);
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
                if spell
                    .body
                    .instructions
                    .iter()
                    .any(|op| matches!(op, Instruction::Unearth | Instruction::Ninjutsu))
                {
                    return Err(DefinitionError::InvalidInstruction);
                }
                if spell.body.instructions.iter().any(|instruction| {
                    matches!(
                        instruction,
                        Instruction::SearchLibrary { filter }
                            if filter.is_haste_and_return() || filter.is_transfigure()
                    ) || matches!(
                        instruction,
                        Instruction::CopyTargetToSource { .. }
                            | Instruction::WeakenOpposingAttackersUntilNextTurn
                            | Instruction::PhaseOtherPlaneswalkersNextEndStep
                            | Instruction::PhaseOutObjects(_)
                            | Instruction::ReturnOneNonlandPerOpponent
                            | Instruction::WeakenDelayedAttacker(_)
                            | Instruction::DrawRevealRemoveLoyalty
                            | Instruction::BoostSource { .. }
                            | Instruction::ResolvePowerAbility(_)
                            | Instruction::BoostSaddledAttacker { .. }
                            | Instruction::Adapt(_)
                            | Instruction::TransformSource
                            | Instruction::TransformSourceAndAttachToTarget
                            | Instruction::ClearSourceCounters(_)
                            | Instruction::PutCountersOnSource { .. }
                            | Instruction::UnattachEquipmentAndProtectHost
                            | Instruction::Unattach
                            | Instruction::Attach(_)
                    )
                }) {
                    return Err(DefinitionError::InvalidInstruction);
                }
                if !self.mana.is_empty() {
                    return Err(DefinitionError::ManaSource);
                }
                if self
                    .activated
                    .iter()
                    .any(|ability| !ability.source_cost.discards_source())
                {
                    return Err(DefinitionError::ActivatedSource);
                }
                if self.triggered.iter().any(|ability| {
                    !matches!(
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
        {
            return Err(DefinitionError::InvalidInstruction);
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
            ) || ability
                .body
                .instructions
                .iter()
                .any(|op| matches!(op, Instruction::DamageFromGrantingPermanent { .. }))
            {
                return Err(DefinitionError::InvalidInstruction);
            }
            if let Some(crate::activated::ActivationRestriction::ControlsSubtypeCount {
                kind, ..
            }) = self.activation_restrictions.get(&ability.id)
                && vocabulary.label(*kind).is_none()
            {
                return Err(DefinitionError::ForeignCreatureType);
            }
            if self.activation_restrictions.get(&ability.id)
                == Some(&crate::activated::ActivationRestriction::AnyPlayer)
                && (ability.source_cost != SourceCost::None
                    || self.loyalty_costs.contains_key(&ability.id))
            {
                return Err(DefinitionError::InvalidInstruction);
            }
            if ability.body.instructions.iter().any(|op| {
                matches!(
                    op,
                    Instruction::ExileTopFaceDown | Instruction::RegisterTokenLinkedExile
                )
            }) && !(self.has_type(CardType::Planeswalker)
                && ability.source_cost == SourceCost::None
                && ability.body.target == BodyTarget::None
                && matches!(
                    ability.body.instructions.as_slice(),
                    [
                        Instruction::ExileTopFaceDown,
                        Instruction::CreateTokens { .. },
                        Instruction::RegisterTokenLinkedExile
                    ]
                ))
            {
                return Err(DefinitionError::InvalidInstruction);
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
                    }
                )
            {
                return Err(DefinitionError::InvalidInstruction);
            }
            let fights_fixed_targets =
                ability.body.instructions.last() == Some(&Instruction::FightFixedTargets);
            if ability.body.instructions.iter().any(|op| {
                matches!(
                    op,
                    Instruction::ExileResolvingSpell
                        | Instruction::DivideThreeDamageAmongTargets
                        | Instruction::DamageOrderedTargets { .. }
                )
            }) || (ability
                .body
                .instructions
                .iter()
                .any(|op| matches!(op, Instruction::FixedTargetClause(_)))
                && ability.source_cost != SourceCost::None
                && !(fights_fixed_targets && ability.source_cost == SourceCost::Tap))
            {
                return Err(DefinitionError::InvalidInstruction);
            }
            if matches!(ability.body.target, BodyTarget::FilteredPermanent(filter) if filter.has_power_at_most_x())
                != matches!(
                    ability.source_cost,
                    SourceCost::TapPermanents {
                        count: TapCostCount::ChosenX,
                        ..
                    }
                )
            {
                return Err(DefinitionError::InvalidInstruction);
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
            if ability
                .body
                .instructions
                .contains(&Instruction::ExileTargetUntilSourceLeaves)
            {
                return Err(DefinitionError::InvalidInstruction);
            }
            if let [Instruction::Attach(attach)] = ability.body.instructions.as_slice()
                && (self.permanent().and_then(|value| value.attachment) != Some(attach.kind())
                    || (ability.source_cost != SourceCost::None
                        && !(*attach == crate::attachment::AttachAbility::Equip
                            && matches!(
                                ability.source_cost,
                                SourceCost::SacrificeSelected {
                                    filter: SelectedSacrificeFilter::Creature,
                                    tap_source: false,
                                    from_graveyard: false,
                                    count: 1,
                                }
                            ))))
            {
                return Err(DefinitionError::InvalidInstruction);
            }
            if !ability.mana_cost.valid() {
                return Err(DefinitionError::CastingCost);
            }
            let context = match ability.source_cost {
                SourceCost::Counter { kind, .. } if !crate::effects::CounterKind::PERMANENT.contains(&kind) => {
                    return Err(DefinitionError::InvalidInstruction);
                }
                SourceCost::Counter { delta: CounterCostDelta::Remove(_) | CounterCostDelta::RemoveAll, .. } => {
                    BodyContext::Activated { named_counter_cost: true }
                }
                SourceCost::ExileGraveyard { filter: GraveyardExileCost::Creature { pay_mana_cost: true }, .. }
                | SourceCost::Discard { mode: DiscardCostMode::WholeHand, .. } => BodyContext::ACTIVATED,
                SourceCost::SacrificeSelected {
                    filter: SelectedSacrificeFilter::Creature | SelectedSacrificeFilter::OtherCreature,
                    count: 1,
                    from_graveyard: false,
                    ..
                }
                | SourceCost::PayLife {
                    sacrifice: Some(SelectedSacrificeFilter::Creature | SelectedSacrificeFilter::OtherCreature),
                    ..
                } => BodyContext::SacrificedCreature,
                SourceCost::SacrificeSelected { count: 1, from_graveyard: false, .. } => BodyContext::SacrificedPermanent,
                _ if matches!(ability.body.target, BodyTarget::FilteredPermanent(filter) if filter.requires_same_name_as_source()) =>
                    BodyContext::ACTIVATED,
                _ if ability.body.instructions.iter().any(|op| {
                    matches!(op, Instruction::DrawEachPlayer { amount } if amount.ability_source_counter_kind().is_some())
                        || matches!(op, Instruction::DamageAmount(amount) if amount.ability_source_counter_kind().is_some())
                        || matches!(op, Instruction::ModifyGroupByAmount { amount: ValueExpr::ObjectStat { subject: ValueObjectRef::AbilitySource, stat: ObjectStatKind::Power }, .. })
                        || matches!(op,
                            Instruction::PutNamedCounterOnSource { .. }
                            | Instruction::TapEnchantedCreature
                            | Instruction::ExileTargetWithLink(LinkedExileGroup::Helvault)
                            | Instruction::ExchangeLifeWithSourceToughness
                            | Instruction::ShuffleAbilitySourceIntoLibrary
                            | Instruction::GrantGraveyardCastAndWatchAbilities
                            | Instruction::DamageFromPower(PowerDamageSource::TargetCreatureToAbilitySource)
                            | Instruction::CreateContinuousEffect { duration: ResolvedDuration::WhileControllingAbilitySource, .. }
                        )
                }) => BodyContext::ACTIVATED,
                _ if ability.body.instructions.contains(&Instruction::ExileTargetAsAbilityDonor)
                    || matches!(ability.body.target, BodyTarget::FilteredPermanent(filter) if filter.is_creature_or_other_planeswalker()) => BodyContext::ACTIVATED,
                _ if ability.body.instructions == [Instruction::PhaseOtherPlaneswalkersNextEndStep] || ability.body.instructions.iter().any(|op| matches!(op, Instruction::AbilitySourceControllerLosesLife(_))) => BodyContext::ACTIVATED,
                _ => BodyContext::Unbound,
            };
            ability.body.validate_in(vocabulary, context)?;
            if matches!(
                ability.source_cost,
                SourceCost::ExileGraveyard {
                    filter: GraveyardExileCost::Creature {
                        pay_mana_cost: true
                    },
                    ..
                }
            ) || ability
                .body
                .instructions
                .contains(&Instruction::CopyCreatureExiledForAbility)
            {
                if !matches!(
                    ability.source_cost,
                    SourceCost::ExileGraveyard {
                        filter: GraveyardExileCost::Creature {
                            pay_mana_cost: true
                        },
                        ..
                    }
                ) || ability.body.instructions != [Instruction::CopyCreatureExiledForAbility]
                    || ability.mana_cost != ManaCost::free()
                {
                    return Err(DefinitionError::InvalidInstruction);
                }
                continue;
            }
            if matches!(ability.source_cost, SourceCost::GenericX { .. }) {
                continue;
            }
            if matches!(ability.source_cost, SourceCost::Counter { .. }) {
                continue;
            }
            if ability.body.instructions == [Instruction::DestroyNonlandsAtSourceChargeCount] {
                if ability.source_cost != SourceCost::TapSacrifice {
                    return Err(DefinitionError::InvalidInstruction);
                }
                continue;
            }
            if matches!(ability.source_cost, SourceCost::Mill { .. }) {
                if ability.body.target != BodyTarget::None
                    || !matches!(
                        ability.body.instructions.as_slice(),
                        [Instruction::AddMana(EffectManaYield::Fixed(_))]
                    )
                {
                    return Err(DefinitionError::InvalidInstruction);
                }
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
                if ability.body.target != BodyTarget::SingleSpell(SpellFilter::any())
                    || ability.body.instructions != [Instruction::CounterSpell]
                {
                    return Err(DefinitionError::InvalidInstruction);
                }
                continue;
            }
            if ability.body.instructions == [Instruction::Encore] {
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
                if self.permanent().is_none() {
                    return Err(DefinitionError::InvalidInstruction);
                }
                continue;
            }
            if matches!(
                ability.source_cost,
                SourceCost::SacrificeSelected {
                    from_graveyard: true,
                    ..
                }
            ) {
                if !matches!(self.kind, ProgramKind::Permanent(_))
                    || ability.body.target != BodyTarget::None
                    || ability.body.instructions != [Instruction::ReturnSourceGraveyardToHand]
                {
                    return Err(DefinitionError::InvalidInstruction);
                }
                continue;
            }
            if ability
                .body
                .instructions
                .iter()
                .any(|op| matches!(op, Instruction::ReturnSourceGraveyardToHand))
            {
                return Err(DefinitionError::InvalidInstruction);
            }
            if matches!(
                ability.source_cost,
                SourceCost::GraveyardSelf { unearth: false }
            ) {
                if !matches!(self.kind, ProgramKind::Permanent(_))
                    || ability.body.target != BodyTarget::None
                    || !matches!(
                        ability.body.instructions.as_slice(),
                        [Instruction::ReturnSourceGraveyardToBattlefield { .. }]
                    )
                {
                    return Err(DefinitionError::InvalidInstruction);
                }
                continue;
            }
            if ability
                .body
                .instructions
                .iter()
                .any(|op| matches!(op, Instruction::ReturnSourceGraveyardToBattlefield { .. }))
            {
                return Err(DefinitionError::InvalidInstruction);
            }
            let enters_from_zone = match ability.source_cost {
                SourceCost::GraveyardSelf { unearth: true } => Some(Instruction::Unearth),
                SourceCost::Ninjutsu { .. } => Some(Instruction::Ninjutsu),
                _ => None,
            };
            if let Some(operation) = enters_from_zone {
                if !matches!(self.kind, ProgramKind::Permanent(_))
                    || ability.body.target != BodyTarget::None
                    || ability.body.instructions != [operation]
                {
                    return Err(DefinitionError::InvalidInstruction);
                }
            } else if ability
                .body
                .instructions
                .iter()
                .any(|op| matches!(op, Instruction::Unearth | Instruction::Ninjutsu))
            {
                return Err(DefinitionError::InvalidInstruction);
            } else if ability.source_cost
                == SourceCost::HandDiscardSelf(HandDiscardKeyword::Reinforce)
            {
                if ability.body.target != BodyTarget::SingleCreature
                    || !matches!(ability.body.instructions.as_slice(),
                    [Instruction::PutPowerToughnessCounters { counters }] if counters.plus_one.is_positive() && counters.minus_one == 0.into())
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
                if ability.body.permanent_target_bounds().is_some()
                    || ability.body.ordered_any_target_bounds().is_some()
                {
                    return Err(DefinitionError::InvalidInstruction);
                }
            } else if let Some((kind, _)) = ability.source_cost.power_cost() {
                if ability.body.instructions.as_slice() != [Instruction::ResolvePowerAbility(kind)]
                    || ability.mana_cost != ManaCost::free()
                {
                    return Err(DefinitionError::InvalidInstruction);
                }
            } else if ability.body.instructions.as_slice()
                == [Instruction::UnattachEquipmentAndProtectHost]
            {
                if !self.has_type(CardType::Creature) || ability.source_cost != SourceCost::None {
                    return Err(DefinitionError::InvalidInstruction);
                }
            } else if ability.body.instructions.as_slice() == [Instruction::Unattach] {
                if self.permanent().and_then(|def| def.attachment)
                    != Some(crate::attachment::AttachmentKind::Equipment)
                    || ability.source_cost != SourceCost::None
                {
                    return Err(DefinitionError::InvalidInstruction);
                }
            } else if self.activation_restrictions.get(&ability.id)
                == Some(&crate::activated::ActivationRestriction::PowerUp)
            {
                if ability.body.target != BodyTarget::None
                    || !matches!(
                        ability.body.instructions.as_slice(),
                        [Instruction::PutCountersOnSource { .. }]
                            | [
                                Instruction::PutCountersOnSource { .. },
                                Instruction::DrawController { .. }
                            ]
                    )
                {
                    return Err(DefinitionError::InvalidInstruction);
                }
            } else if matches!(
                ability.body.instructions.as_slice(),
                [Instruction::ModifyGroupUntilEndOfTurn { .. }
                    | Instruction::SetControlledCreatureSizeToHand
                    | Instruction::ModifyGroupByAmount { .. }]
            ) {
                if ability.body.target != BodyTarget::None
                    || !matches!(
                        ability.source_cost,
                        SourceCost::Tap
                            | SourceCost::None
                            | SourceCost::Sacrifice
                            | SourceCost::TapSacrifice
                            | SourceCost::SacrificeSelected {
                                from_graveyard: false,
                                ..
                            }
                            | SourceCost::Discard {
                                mode: DiscardCostMode::SelectedOne,
                                ..
                            }
                            | SourceCost::PayLife {
                                sacrifice: Some(_),
                                ..
                            }
                    )
                {
                    return Err(DefinitionError::InvalidInstruction);
                }
            } else if matches!(
                ability.body.instructions.as_slice(),
                [Instruction::Adapt(_)]
            ) {
                if !self.has_type(CardType::Creature) || ability.source_cost != SourceCost::None {
                    return Err(DefinitionError::InvalidInstruction);
                }
            } else if matches!(
                ability.body.instructions.as_slice(),
                [Instruction::BoostSource { .. }]
            ) {
                if !self.has_type(CardType::Creature)
                    || !matches!(
                        ability.source_cost,
                        SourceCost::None
                            | SourceCost::PayLife {
                                tap_source: false,
                                ..
                            }
                            | SourceCost::Tap
                            | SourceCost::SacrificeSelected {
                                from_graveyard: false,
                                ..
                            }
                            | SourceCost::Discard {
                                mode: DiscardCostMode::SelectedOne,
                                ..
                            }
                    )
                {
                    return Err(DefinitionError::InvalidInstruction);
                }
            } else if ability.body.target == BodyTarget::None
                && !fights_fixed_targets
                && !(self.has_type(CardType::Creature)
                    && ability.source_cost == SourceCost::Tap
                    && matches!(ability.body.instructions.as_slice(),
                    [Instruction::MillEachPlayer(_),Instruction::ExileMilledCreatures {..},Instruction::CreateTokens {creations}] if creations.iter().all(|creation|creation.base_size==Some(crate::token::TokenSize::AtCreation(ValueExpr::event(EventValueKind::ExiledMilledCreaturePower))))))
                && !self.loyalty_costs.contains_key(&ability.id)
                && !matches!(
                    ability.body.instructions.as_slice(),
                    [Instruction::PutCountersOnSource { .. }]
                )
                && !(self.permanent().is_some()
                    && matches!(
                        ability.body.instructions.as_slice(),
                        [Instruction::SearchLibrary { .. }]
                    ))
                && !(ability.source_cost == SourceCost::Sacrifice
                    && matches!(
                        ability.body.instructions.as_slice(),
                        [Instruction::RegenerateControlledSubtype(_)]
                    ))
                && !matches!(
                    ability.body.instructions.as_slice(),
                    [Instruction::PutChargeCounterOnSource]
                )
                && !(self.has_type(CardType::Land)
                    && matches!(
                        ability.body.instructions.as_slice(),
                        [Instruction::PutStorageCounterOnSource]
                    ))
                && !(self.permanent().is_some()
                    && ability.body.instructions.iter().all(|operation| {
                        matches!(
                            operation,
                            Instruction::LookTopReveal {
                                filter: LookTopFilter::SourceChosenCreatureType,
                                ..
                            } | Instruction::SuppressGroupKeywords { .. }
                                | Instruction::ExileDefeatedSiege
                                | Instruction::ExileSourceAndGainLife(_)
                                | Instruction::ExileAbilitySource
                                | Instruction::ReturnSourceToHand
                                | Instruction::UntapAbilitySource
                                | Instruction::TapAbilitySource
                                | Instruction::RegenerateSource
                                | Instruction::GainControllerLife { .. }
                                | Instruction::DrainTargetAndGain(_)
                                | Instruction::AdjustControllerLife { .. }
                                | Instruction::RequireAttackSourceNextTurn
                                | Instruction::ChooseAttackDirection
                                | Instruction::AdditionalLoyaltyActivation
                                | Instruction::PayControllerLife(_)
                                | Instruction::PutEquipmentFromHandOrGraveyard
                                | Instruction::EquipNewToken
                                | Instruction::Encore
                                | Instruction::GrantNewTokensHaste
                                | Instruction::RepeatOpponentLifeLoss
                                | Instruction::OpponentsLoseAndControllerGains(_)
                                | Instruction::AdjustEachOpponentLife { .. }
                                | Instruction::FlipCoin { .. }
                                | Instruction::GoadOtherPlayersCreatures
                                | Instruction::AdditionalCombat { .. }
                                | Instruction::AdditionalCombatAfterUntappingAttackers
                                | Instruction::TakeExtraTurn
                                | Instruction::PreventDamageThisTurn(_)
                                | Instruction::MoveGroup { .. }
                                | Instruction::PutTargetInLibrary(_)
                                | Instruction::PutOwnHandCardOnLibraryBottom
                                | Instruction::PutGroupOnLibraryBottom(_)
                                | Instruction::ExileGraveyardTarget
                                | Instruction::ExileCapturedGraveyardCard
                                | Instruction::GrantGraveyardCastAndWatchAbilities
                                | Instruction::GainCastCardAbilities { .. }
                                | Instruction::ExileCardsFromGraveyard { .. }
                                | Instruction::ReturnOwnedCommandersToHand
                                | Instruction::ExileTopForFreePlay
                                | Instruction::ExileTargetUntilNonlandForFreeCast
                                | Instruction::ExileTopRepeatOnPermanentEntry
                                | Instruction::ShuffleEachHandAndGraveyard
                                | Instruction::EndControllerTurn
                                | Instruction::ShuffleOwnLibrary
                                | Instruction::ShuffleOwnGraveyardIntoLibrary
                                | Instruction::ShuffleTargetGraveyardCards { .. }
                                | Instruction::MayTurnControlledCreatureFaceUp
                                | Instruction::ExploreSource
                                | Instruction::MayRevealLibraryTop
                                | Instruction::ExileCreatureAtMostRevealedPower
                                | Instruction::CastLinkedSpellMatchingX
                                | Instruction::DestroyDamagedPlayersNonlandsAtX
                                | Instruction::PutChosenXChargeCounters
                                | Instruction::DestroyNonlandsAtSourceChargeCount
                                | Instruction::AddCounterAndDischargeOnResolution(_)
                                | Instruction::ExileCreaturesAndPlaneswalkersBelowRevealedManaValue
                                | Instruction::ExileAllGraveyards
                                | Instruction::DestroyGroupWithoutRegeneration(_)
                                | Instruction::DestroyGroup(_)
                                | Instruction::ExileOnePerPlayerUntilSourceLeaves
                                | Instruction::DoubleCountersOnTargets
                                | Instruction::ExileCombatDamagedCreature
                                | Instruction::DestroyCapturedDamageSource
                                | Instruction::RemoveCreatureCounters { .. }
                                | Instruction::UntapControlledLands
                                | Instruction::UntapControlledCreatures
                                | Instruction::TapAttackersAndSkipNextUntap
                                | Instruction::AttachOnEntry { .. }
                                | Instruction::ControlledSubtypeDamagesAttacker(_)
                                | Instruction::RevealTopAndRemoveAttackerIfForest
                                | Instruction::DamageCapturedBlocker(_)
                                | Instruction::ChangeControlledPlaneswalkers { .. }
                                | Instruction::ChangeAbilitySource { .. }
                                | Instruction::GiveControlOfAbilitySource
                                | Instruction::UntapAndControlCreatures
                                | Instruction::Proliferate
                                | Instruction::AttachControlledToSource { .. }
                                | Instruction::ReturnDelayedGraveyardToHand(_)
                                | Instruction::LoseLifeAndScryForSubtype(_)
                                | Instruction::ExileForBlackToken { .. }
                                | Instruction::ExileForSubtypeToken(_)
                                | Instruction::GiveEquippedCreatureToUpkeepPlayer
                                | Instruction::CopyCreatureExiledForAbility
                                | Instruction::DamageFromGrantingPermanent { .. }
                                | Instruction::CopyEntrantAsArtifact
                                | Instruction::ReplaceSourceCreatedTokens
                                | Instruction::CopyEntrantOnceEachTurn
                                | Instruction::CopyCreatureAsToken
                                | Instruction::CopyDifferentlyNamedTokens
                                | Instruction::DamagePlayersHalfLife
                                | Instruction::DamageGroup { .. }
                                | Instruction::DamageTargetControllersCreatures { .. }
                                | Instruction::DrawAndDamageOpponents { .. }
                                | Instruction::Adapt(_)
                                | Instruction::TransformSource
                                | Instruction::TransformSourceAndAttachToTarget
                                | Instruction::TransformSubtype(_)
                                | Instruction::ClearSourceCounters(_)
                                | Instruction::PutCountersOnSource { .. }
                                | Instruction::MillEachPlayer(_)
                                | Instruction::PutCountersOnControlledCreatures
                                | Instruction::PutCountersOnOtherCreatures
                                | Instruction::PutCountersOnControlledSubtype(_)
                                | Instruction::ReturnRandomGraveyardCards { .. }
                                | Instruction::TapEnchantedCreature
                                | Instruction::ModifyGroupUntilEndOfTurn { .. }
                                | Instruction::PutNamedCounterOnSource { .. }
                                | Instruction::ExchangeLifeWithSourceToughness
                                | Instruction::ShuffleAbilitySourceIntoLibrary
                                | Instruction::RevealToNameAfterSourceShuffle(_)
                                | Instruction::BranchIfFalse { .. }
                                | Instruction::Jump { .. }
                                | Instruction::GrantControllerFlashUntilCleanup
                                | Instruction::DrawEachPlayer { .. }
                                | Instruction::AbilitySourceControllerLosesLife(_)
                                | Instruction::DrawPlayersControllingName(_)
                                | Instruction::DrawController { .. }
                                | Instruction::CreateTokensToFight(_)
                                | Instruction::FightPairedCreatures(_)
                                | Instruction::OfferOpponentsDamage
                                | Instruction::TemptWithTokens { .. }
                                | Instruction::CreateTokens { .. }
                                | Instruction::CreateAttackingTokens { .. }
                                | Instruction::Scry {
                                    player: ResolvingPlayer::Controller,
                                    ..
                                }
                                | Instruction::Surveil {
                                    player: ResolvingPlayer::Controller,
                                    ..
                                }
                                | Instruction::Sacrifice {
                                    players: AffectedPlayers::One(ResolvingPlayer::Controller)
                                        | AffectedPlayers::EachOpponent
                                        | AffectedPlayers::EachOtherPlayer
                                        | AffectedPlayers::EachPlayer,
                                    ..
                                }
                                | Instruction::Discard {
                                    players: AffectedPlayers::One(ResolvingPlayer::Controller)
                                        | AffectedPlayers::EachOpponent
                                        | AffectedPlayers::EachOtherPlayer
                                        | AffectedPlayers::EachPlayer,
                                    ..
                                }
                        )
                    }))
            {
                return Err(DefinitionError::TargetlessActivation);
            }
        }
        for ability in &self.triggered {
            if ability.body.instructions.iter().any(|op| {
                matches!(
                    op,
                    Instruction::FixedTargetClause(_)
                        | Instruction::ExileResolvingSpell
                        | Instruction::CreateReflexiveDiscardDamage
                )
            }) {
                return Err(DefinitionError::InvalidInstruction);
            }
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
        if self.condition.intervening_if == Some(TriggerCondition::ControlsMeldPair)
            && !matches!(
                self.body.instructions.as_slice(),
                [
                    Instruction::ChooseMeldPartner(_),
                    Instruction::ExileMeldPair,
                    Instruction::MeldExiledPair(_)
                ]
            )
        {
            return Err(DefinitionError::InvalidInstruction);
        }
        if let TriggerEvent::Keyword(TriggerKeywordEvent::Soulbond { entrant_only }) =
            self.condition.event
            && self.body.instructions.as_slice() != [Instruction::PairWithCreature { entrant_only }]
        {
            return Err(DefinitionError::InvalidInstruction);
        }
        if let Some(TriggerCondition::HandSizeBelow(required)) = self.condition.intervening_if
            && (required == 0
                || self.grouping != TriggerGrouping::EachOccurrence
                || self.body.target != BodyTarget::None
                || !matches!(self.body.instructions.as_slice(), [Instruction::DrawController { amount: ValueExpr::PlayerStat(PlayerStatExpr::CardsShortOfHandSize(amount)) }] if *amount == required))
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
            && (!crate::effects::CounterKind::PERMANENT.contains(&kind)
                || self.grouping != TriggerGrouping::EachOccurrence)
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
            .and_then(InterveningCondition::from_trigger_condition)
        {
            let needs_each_occurrence = matches!(
                cond,
                InterveningCondition::OwnCommander
                    | InterveningCondition::GreatestArtifact
                    | InterveningCondition::OpponentMoreLands
                    | InterveningCondition::Monarch { .. }
            );
            let needs_no_target = !matches!(
                cond,
                InterveningCondition::OwnCommander | InterveningCondition::NoLoyaltyActivation
            );
            if (needs_each_occurrence && self.grouping != TriggerGrouping::EachOccurrence)
                || (needs_no_target && self.body.target != BodyTarget::None)
                || self.body.instructions.first() != Some(&Instruction::RequireCondition(cond))
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
            .any(|op| matches!(op, Instruction::Unearth | Instruction::Ninjutsu))
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
