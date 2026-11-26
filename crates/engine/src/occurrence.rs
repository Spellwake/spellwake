//! Transition-local semantic facts and owned ordinary-trigger captures.
//! Journals never own accepted game history or drive discovery by replay.

pub use crate::characteristics::CardType;
#[cfg(test)]
use crate::mana::ManaAbilityId;
use crate::{
    activated::ActivatedAbilityId,
    characteristics::{
        CharacteristicState, Color, ContinuousEffectId, EvaluatedCharacteristics, KeywordAbility,
        PowerToughness, PowerToughnessCounters, PrintedAbilityRoster, PrintedPowerToughness,
        PrintedText, ResolvedContinuousEffect,
    },
    checks::{CheckBoundary, CheckEventId},
    combat::{
        AssignmentOrigin, AttackDeclaration, AttackingCreature, BlockDeclaration, BlockStatus,
        BlockingCreature, CombatDamageAssignment, CombatState, Combatant, DeclarationOrigin,
    },
    damage::{
        AppliedDamage, DamageAttemptId, DamageBatchId, DamageEffectCreation, DamageKind,
        DamageMultiplication, DamageOrigin, DamageRecipient, PreventionApplication,
    },
    life::{LifeChange, LifeState},
    mana::{ManaPaymentRecord, ManaProductionRecord, ManaType},
    object_identity::{
        ControllerChange, GameObject, GameObjects, ObjectId, ObjectStatusChange, ObjectZone,
    },
    priority::SeatId,
    program::{
        AttackTriggerMode, CreatureTypeSlot, ExecutableBody, ObjectFilter, PlayerRelation,
        PlayerTriggerAction, SpellFilter, TriggerCondition, TriggerEvent, TriggerGrouping,
        TriggerKeywordEvent, TriggeredAbility, TriggeredAbilityId,
    },
    shuffle::ShuffleRecord,
    stack_window::{SpellCopyObject, StackEntryId, StackKind, StackWindow},
    targets::TargetChoice,
    turn::{TurnPosition, TurnScheduler},
};
use spellwake_contracts::{RulesInteger, creature_types::CreatureTypeId};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

mod combat_draw;
pub(crate) use combat_draw::{CombatDrawSource, CombatDrawSubject};
mod causes;
pub use causes::{TriggerCauseKey, TriggerCauseMember, TriggerCauseSet};
mod matching;
use matching::{match_trigger_event, player_matches};

#[cfg(test)]
mod transport_tests;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct EventId(u64);
impl EventId {
    pub fn sequence(self) -> u64 {
        self.0
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct OccurrenceId {
    pub event: EventId,
    pub index: u64,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct TriggerInstanceId(u64);
impl TriggerInstanceId {
    pub fn sequence(self) -> u64 {
        self.0
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct PlacementWaveId(u64);
impl PlacementWaveId {
    pub fn sequence(self) -> u64 {
        self.0
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TriggerError {
    EventIdentityExhausted,
    OccurrenceIndexExhausted,
    InstanceIdentityExhausted,
    WaveIdentityExhausted,
    EmptyEvent,
    /// An internally formed group is empty, mixed, duplicated or has a wrong key.
    InvalidTriggerCauses,
    /// A causal collection cannot be represented by addressable native storage.
    TriggerCauseCapacity,
    WrongOrderFrame,
    /// A placement-target frame differs from the exact current pending choice.
    WrongTargetFrame,
    InvalidTriggerOrder,
    /// An internally fabricated view disagrees with its evaluated roster.
    InvalidWorld,
}

/// Bounded trigger-time information from one evaluated physical cut.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SupportedAppearance {
    /// Actual casting player and whether they cast this stack incarnation from their hand.
    pub cast_origin: Option<(SeatId, bool)>,
    /// X on this exact stack incarnation, including a copy of a permanent spell.
    pub spell_x: Option<RulesInteger>,
    /// Kicker payment counts on this exact stack incarnation (e.g. Squad, Multikicker).
    pub kicker_choices: BTreeMap<usize, RulesInteger>,
    /// Current attachment host captured at the event cut.
    pub attached_to: Option<crate::targets::TargetRef>,
    /// This exact permanent has resolved a saddle ability this turn.
    pub saddled: bool,
    /// Physical owner if this object is a commander in this game.
    pub commander_owner: Option<SeatId>,
    pub object: GameObject,
    pub copiable_names: spellwake_contracts::names::ObjectNames,
    pub current_names: spellwake_contracts::names::ObjectNames,
    pub copiable_supertypes: BTreeSet<crate::program::Supertype>,
    pub current_supertypes: BTreeSet<crate::program::Supertype>,
    pub copiable_card_types: BTreeSet<CardType>,
    pub copiable_creature_subtypes: BTreeSet<CreatureTypeId>,
    pub copiable_land_subtypes: BTreeSet<crate::land::LandType>,
    pub copiable_spell_subtypes: BTreeSet<crate::program::SpellSubtype>,
    pub copiable_artifact_subtypes: BTreeSet<crate::program::ArtifactSubtype>,
    pub copiable_enchantment_subtypes: BTreeSet<crate::program::EnchantmentSubtype>,
    pub copiable_planeswalker_subtypes: BTreeSet<crate::program::PlaneswalkerSubtype>,
    pub types: BTreeSet<CardType>,
    pub creature_subtypes: BTreeSet<CreatureTypeId>,
    pub land_subtypes: BTreeSet<crate::land::LandType>,
    pub spell_subtypes: BTreeSet<crate::program::SpellSubtype>,
    pub artifact_subtypes: BTreeSet<crate::program::ArtifactSubtype>,
    pub enchantment_subtypes: BTreeSet<crate::program::EnchantmentSubtype>,
    pub planeswalker_subtypes: BTreeSet<crate::program::PlaneswalkerSubtype>,
    pub intrinsic_mana: BTreeSet<crate::land::BasicLandType>,
    pub colors: BTreeSet<Color>,
    pub abilities: PrintedAbilityRoster,
    /// Layer-3 printed definitions at this exact cut, including removed printed
    /// abilities; the separate roster determines which ones actually exist.
    pub printed_text: PrintedText,
    /// Surviving printed and granted keywords at this appearance.
    pub keywords: BTreeSet<KeywordAbility>,
    /// Boundary preceding the captured upkeep for an echo source. The acquired control epoch is on its physical appearance.
    pub previous_upkeep: Option<crate::object_identity::ControlEpoch>,
    pub time_counters: Option<RulesInteger>,
    pub fade_counters: Option<RulesInteger>,
    pub renowned: bool,
    /// This exact permanent has become monstrous (CR 701.31).
    pub monstrous: bool,
    /// This exact permanent is suspected (CR 701.58).
    pub suspected: bool,
    /// This exact permanent is goaded (CR 701.38).
    pub goaded: bool,
    /// This exact Case permanent is solved (CR 719.3).
    pub solved: bool,
    /// This exact permanent is prepared.
    pub prepared: bool,
    /// This exact permanent was exerted this turn (CR 701.39).
    pub exerted: bool,
    /// This object's copiable definition has an Adventure alternative face (CR 715).
    pub has_adventure: bool,
    pub has_x_in_cost: bool,
    pub has_toxic: bool,
    pub has_no_abilities: bool,
    pub toxic_value: Option<RulesInteger>,
    /// Whether the creature had the granted evasion ability at this cut.
    pub granted_unblockable: bool,
    /// Current independent exalted grants at this cut.
    pub granted_exalted: BTreeMap<crate::characteristics::EffectInstanceKey, u16>,
    /// Ordinary triggers keyed by the effect granting each instance at this cut.
    pub granted_triggers:
        BTreeMap<crate::characteristics::EffectInstanceKey, Vec<TriggeredAbility>>,
    /// Surviving combat keyword instances indexed by the effect that granted them.
    pub granted_combat_keywords: BTreeMap<
        crate::characteristics::EffectInstanceKey,
        Vec<crate::program::CombatTriggerKeyword>,
    >,
    pub copiable_power_toughness: Option<PrintedPowerToughness>,
    pub battlefield_counters: Option<PowerToughnessCounters>,
    /// All counter kinds on the battlefield incarnation at the captured cut.
    pub battlefield_total_counters: Option<RulesInteger>,
    pub battlefield_marked_damage: Option<RulesInteger>,
    /// Current numeric values in this appearance's exact zone (CR 208.3).
    pub current_power_toughness: Option<PowerToughness>,
}
/// Owned before/after values from the same complete operation boundary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CharacteristicAppearanceChange {
    pub object: ObjectId,
    pub before: SupportedAppearance,
    pub after: SupportedAppearance,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MarkedDamageRemoval {
    pub object: ObjectId,
    pub previous: RulesInteger,
}
impl SupportedAppearance {
    pub(crate) fn synthetic_emblem(controller: SeatId) -> Self {
        let empty = crate::program::Program::enchantment(None);
        Self {
            cast_origin: None,
            spell_x: None,
            kicker_choices: BTreeMap::new(),
            attached_to: None,
            saddled: false,
            commander_owner: None,
            object: GameObject::synthetic_emblem(controller),
            copiable_names: spellwake_contracts::names::ObjectNames::nameless(),
            current_names: spellwake_contracts::names::ObjectNames::nameless(),
            copiable_supertypes: BTreeSet::new(),
            current_supertypes: BTreeSet::new(),
            copiable_card_types: BTreeSet::new(),
            copiable_creature_subtypes: BTreeSet::new(),
            copiable_land_subtypes: BTreeSet::new(),
            copiable_spell_subtypes: BTreeSet::new(),
            copiable_artifact_subtypes: BTreeSet::new(),
            copiable_enchantment_subtypes: BTreeSet::new(),
            copiable_planeswalker_subtypes: BTreeSet::new(),
            types: BTreeSet::new(),
            creature_subtypes: BTreeSet::new(),
            land_subtypes: BTreeSet::new(),
            spell_subtypes: BTreeSet::new(),
            artifact_subtypes: BTreeSet::new(),
            enchantment_subtypes: BTreeSet::new(),
            planeswalker_subtypes: BTreeSet::new(),
            intrinsic_mana: BTreeSet::new(),
            colors: BTreeSet::new(),
            abilities: PrintedAbilityRoster::empty_emblem(ObjectId::SENTINEL),
            printed_text: PrintedText::from_program(&empty),
            keywords: BTreeSet::new(),
            previous_upkeep: None,
            time_counters: None,
            fade_counters: None,
            renowned: false,
            monstrous: false,
            suspected: false,
            goaded: false,
            solved: false,
            prepared: false,
            exerted: false,
            has_adventure: false,
            has_x_in_cost: false,
            has_toxic: false,
            has_no_abilities: false,
            toxic_value: None,
            granted_unblockable: false,
            granted_exalted: BTreeMap::new(),
            granted_triggers: BTreeMap::new(),
            granted_combat_keywords: BTreeMap::new(),
            copiable_power_toughness: None,
            battlefield_counters: None,
            battlefield_total_counters: None,
            battlefield_marked_damage: None,
            current_power_toughness: None,
        }
    }
    /// CR 702.30a checks acquisition since the start of the controller's previous upkeep.
    pub(crate) fn echo_condition(&self) -> bool {
        self.object
            .control_epoch()
            .is_some_and(|epoch| self.previous_upkeep.is_none_or(|cut| epoch > cut))
    }
    pub fn is_creature(&self) -> bool {
        self.types.contains(&CardType::Creature)
    }
}

/// One checked physical spell at its actual completed casting cut (CR 601.2i).
/// The immutable capture survives later countering, resolution or source departure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CastOccurrence {
    cast_from_graveyard: bool,
    cast_from_exile: bool,
    cast_from_hand: bool,
    cast_from_library: Option<SeatId>,
    pub(crate) ordinal_this_turn: Option<RulesInteger>,
    mana_value: RulesInteger,
    pub(crate) copy_source: Option<Box<crate::effects::cast_copies::CastCopySource>>,
    cast_original_object: Option<ObjectId>,
    spent_for_increment: Option<RulesInteger>,
    total_mana_spent: RulesInteger,
    entry: StackEntryId,
    controller: SeatId,
    spell: SupportedAppearance,
}
impl CastOccurrence {
    /// Mana value of the resulting stack spell when the cast completed.
    pub fn ordinal_this_turn(&self) -> Option<&RulesInteger> {
        self.ordinal_this_turn.as_ref()
    }
    pub fn mana_value(&self) -> &RulesInteger {
        &self.mana_value
    }
    /// Actual mana spent when an increment ability could observe this cast.
    pub fn spent_for_increment(&self) -> Option<&RulesInteger> {
        self.spent_for_increment.as_ref()
    }
    /// Total mana spent to cast this spell at cast completion (CR 601.2i).
    pub fn total_mana_spent(&self) -> &RulesInteger {
        &self.total_mana_spent
    }

    /// The actual canonical stack entry created by this cast.
    pub fn entry(&self) -> StackEntryId {
        self.entry
    }
    /// The spell's controller when casting completed, also its actual caster here.
    pub fn controller(&self) -> SeatId {
        self.controller
    }
    /// The evaluated new stack incarnation, independently of the prior hand object.
    pub fn spell(&self) -> &SupportedAppearance {
        &self.spell
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpeningMovePhase {
    ReturnForMulligan,
    ChargedBottom,
    PregameBattlefield,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpeningShufflePhase {
    Initial,
    Mulligan,
}
/// Represented simultaneous movement requirements, distinct from destruction
/// replacements or controller choices. Further SBA families qualify separately.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum StateBasedMoveReason {
    IllegalAura,
    ZeroToughness,
    /// A planeswalker with no loyalty counters (CR 704.5i).
    ZeroLoyalty,
    /// A battle with zero defense, after any pending Siege trigger leaves the stack.
    ZeroDefense,
    /// No active player can protect this battle (CR 310.11).
    NoBattleProtector,
    /// The final chapter has been reached and all chapter abilities have left the stack.
    CompletedSaga,
    LethalDestruction,
    DeathtouchDestruction,
    Legend,
    World,
}
impl StateBasedMoveReason {
    const fn bit(self) -> u16 {
        match self {
            Self::IllegalAura => 32,
            Self::ZeroToughness => 1,
            Self::ZeroLoyalty => 64,
            Self::ZeroDefense => 256,
            Self::NoBattleProtector => 512,
            Self::CompletedSaga => 128,
            Self::LethalDestruction => 2,
            Self::DeathtouchDestruction => 16,
            Self::Legend => 4,
            Self::World => 8,
        }
    }
}
/// Nonempty closed cause set. Only singleton construction and union are exposed;
/// the private bit representation cannot be authored as an empty/unknown wire set.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StateBasedMoveReasons(u16);
impl StateBasedMoveReasons {
    /// One applicable typed reason establishes the nonempty invariant.
    pub const fn new(reason: StateBasedMoveReason) -> Self {
        Self(reason.bit())
    }
    /// Whether this exact represented cause participates in the same movement.
    pub const fn contains(self, reason: StateBasedMoveReason) -> bool {
        self.0 & reason.bit() != 0
    }
    /// Stable rule-family order, independent of insertion/observation order.
    pub fn iter(self) -> impl Iterator<Item = StateBasedMoveReason> {
        [
            StateBasedMoveReason::ZeroToughness,
            StateBasedMoveReason::ZeroLoyalty,
            StateBasedMoveReason::ZeroDefense,
            StateBasedMoveReason::NoBattleProtector,
            StateBasedMoveReason::CompletedSaga,
            StateBasedMoveReason::LethalDestruction,
            StateBasedMoveReason::DeathtouchDestruction,
            StateBasedMoveReason::Legend,
            StateBasedMoveReason::World,
            StateBasedMoveReason::IllegalAura,
        ]
        .into_iter()
        .filter(move |reason| self.contains(*reason))
    }
    pub(crate) fn insert(&mut self, reason: StateBasedMoveReason) {
        self.0 |= reason.bit();
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ZoneReason {
    /// An object is exiled when its controller has left the game (CR 800.4a, 800.4c).
    PlayerDeparture,
    CascadeExile,
    CascadeBottom,
    /// A madness trigger resolved without casting the exiled card.
    MadnessDeclined,
    Ingest {
        player: SeatId,
    },
    CommanderReturn,
    Plot,
    Foretell,
    Suspend,
    DelveCost,
    /// Other graveyard card exiled while paying an escape cost.
    EscapeCost,
    AlternativeHandExileCost,
    Surveil {
        player: SeatId,
    },
    Mill {
        player: SeatId,
    },
    Opening(OpeningMovePhase),
    LandPlay,
    CastAnnouncement,
    PermanentResolution,
    ResolutionExit,
    AllTargetsIllegalExit,
    Destroyed,
    /// Exiled by a resolving instruction.
    ExiledByEffect,
    /// A searched card was put into its owner's hand.
    SearchToHand,
    /// A searched library card was put into its owner’s graveyard.
    SearchToGraveyard,
    /// Revealed library cards put into their owner’s graveyard.
    RevealedToGraveyard,
    PutLibraryCohortIntoGraveyard,
    /// A graveyard card moved into its owner’s library for a shuffle.
    ShuffleGraveyardIntoLibrary,
    ShuffleHandAndGraveyardIntoLibrary,
    /// A permanent moves into its owner’s library before shuffling.
    ShufflePermanentIntoLibrary,
    /// An effect put a card or token at the specified end of its owner's library.
    PutIntoOwnersLibrary(crate::program::LibraryPlacement),
    /// Previously exiled land cards move to the bottom in a random order.
    ExiledCardsToLibraryBottom,
    /// A card from a looked-at library segment moved to hand.
    LookTopToHand,
    SearchToExile,
    /// A searched library card entered the battlefield.
    SearchToBattlefield,
    /// A looked-at library card was put onto the battlefield.
    LookTopToBattlefield,
    HandToBattlefield,
    ExileToBattlefield,
    Manifest,
    /// A targeted creature card returned from its controller's graveyard.
    ReturnFromGraveyardToBattlefield,
    ReturnFromSacrifice,
    /// A card returned as the linked source's exile duration ended.
    ReturnFromLinkedExile,
    /// A library creature entered as its controller resolved transfigure.
    TransfigureEntry,
    /// Returned by an unearth ability on resolution.
    NinjutsuEntry,
    UnearthReturn,
    PersistReturn,
    UndyingReturn,
    /// Exiled by the delayed unearth trigger.
    UnearthExile,
    DashReturn,
    WarpExile,
    ReturnToOwnersHand,
    Countered,
    SacrificeCost,
    ExileCost,
    /// Discarded to pay a transmute activation, independent of cycling.
    TransmuteDiscard {
        player: SeatId,
    },
    /// This card was discarded to pay for a particular cycling activation.
    CycleDiscard {
        player: SeatId,
    },
    /// An affected controller's selected-object effect, distinct from payment.
    Sacrifice {
        player: SeatId,
    },
    Draw {
        player: SeatId,
    },
    Discard {
        player: SeatId,
    },
    StateBased(StateBasedMoveReasons),
    /// A disclosed actual mutation/publisher unit, never an ordinary player command.
    MechanismUnit,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ZoneOccurrence {
    /// Other destination objects for the same single permanent departure.
    pub additional_arrivals: Vec<SupportedAppearance>,
    /// This discard was actually replaced by its own madness ability.
    pub madness: bool,
    pub before: SupportedAppearance,
    pub after: SupportedAppearance,
    pub reason: ZoneReason,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatusReason {
    Effect,
    Regeneration,
    CastObjectCost,
    UntapTurn,
    ManaAbilityCost,
    ActivatedAbilityCost,
    AttackDeclaration,
    MechanismUnit,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TriggerRelated {
    MiracleRevealed(ObjectId),
    /// Mana actually paid for the separately targeted graveyard return.
    GraveyardReturnPaid {
        mana_value: RulesInteger,
    },
    /// A nonland discard created this reflexive damage trigger.
    ReflexiveDiscardDamage {
        amount: RulesInteger,
    },
    /// A generalized CR 603.12 reflexive trigger generated during resolution.
    ReflexiveTrigger {
        chosen_x: Option<RulesInteger>,
    },
    CardDrawn {
        player: SeatId,
    },
    PlayerLost(SeatId),
    LeftGame(ObjectId),
    SagaChapter {
        number: u8,
    },
    FinalSagaChapter {
        saga: ObjectId,
        mana_value: RulesInteger,
    },
    SubtypeCombatDamage {
        creature: ObjectId,
        player: SeatId,
        attempt: DamageAttemptId,
        amount: RulesInteger,
    },
    AttachedCreatureDamaged {
        creature: ObjectId,
        attempt: DamageAttemptId,
        amount: RulesInteger,
    },
    DamageToOpponent {
        player: SeatId,
        attempt: DamageAttemptId,
        amount: RulesInteger,
    },
    DamageDealtToPermanent {
        recipient: ObjectId,
        attempt: DamageAttemptId,
        amount: RulesInteger,
    },
    DamageReceived {
        attempt: DamageAttemptId,
        amount: RulesInteger,
    },
    DamageSource(crate::damage::DamageSourceId),
    TargetingEntry(StackEntryId),
    /// A permanent observed as a target, bound without targeting it with the trigger.
    TargetedPermanent(ObjectId),
    CounterSource(ObjectId),
    StateTriggerSource(ObjectId),
    Phased(ObjectId),
    Mutated(ObjectId),
    /// Exact battlefield object from the declaration. It is not a spell or ability target.
    AttackedAlone(ObjectId),
    AttackedWithTotalPower(RulesInteger),
    DeclaredSubtypeAttackers(Vec<ObjectId>),
    /// Exact permanent that attacked, blocked, became blocked, or blocked the trigger source.
    Combatant(ObjectId),
    ManaSource(ObjectId),
    AttackGroup {
        attacked: crate::targets::TargetRef,
        creatures: Vec<ObjectId>,
    },
    CombatDamageToPlaneswalker {
        planeswalker: ObjectId,
        attempt: DamageAttemptId,
        amount: RulesInteger,
    },
    CombatDamageToPlayer {
        source: ObjectId,
        amount: RulesInteger,
        player: SeatId,
        attempt: DamageAttemptId,
    },
    LibraryAction(crate::effects::LibraryAction),
    /// Own the complete causing spell, not a later lookup or a printed-name alias.
    SpellCast(Box<CastOccurrence>),
    SpellCopied(StackEntryId),
    AbilityForCopy(std::sync::Arc<crate::effects::CapturedAbilityCopy>),
    TokenCreated {
        creator: SeatId,
        after: SupportedAppearance,
    },
    LifeChanged(LifeChange),
    ZoneChange {
        before: SupportedAppearance,
        after: SupportedAppearance,
    },
    StatusChange(ObjectStatusChange),
    Discard {
        player: SeatId,
        before: SupportedAppearance,
        after: SupportedAppearance,
    },
    Step {
        position: TurnPosition,
        active: SeatId,
    },
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum PlayerLossReason {
    /// At least 21 combat damage dealt by this same physical commander.
    CommanderCombatDamage(crate::object_identity::CardId),
    NonpositiveLife,
    TenPoisonCounters,
    FailedDraw,
    Conceded,
    Effect,
}
/// A source-attributed poison-counter result. Multiple contributions can share a simultaneous cut.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PoisonContribution {
    pub origin: PoisonOrigin,
    pub source: crate::damage::DamageSourceId,
    pub controller: SeatId,
    pub amount: RulesInteger,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PoisonOrigin {
    Infect(DamageAttemptId),
    Toxic(DamageAttemptId),
    Poisonous(StackEntryId),
    Effect(StackEntryId),
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryCompletion {
    Resolved,
    AllTargetsIllegal,
    /// An intervening-if trigger's condition failed immediately before resolution.
    InterveningIfFalse,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DrawOrigin {
    Opening,
    Turn {
        position: TurnPosition,
    },
    Instruction {
        entry: StackEntryId,
        instruction: usize,
    },
    MechanismUnit,
}
/// Family/context identity is typed; arbitrary strings cannot subscribe as triggers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EventCause {
    TurnFaceUp {
        actor: SeatId,
        source: ObjectId,
        payment: crate::mana::ManaPayment,
    },
    /// Face-up special action paid from hand without creating a stack entry.
    Foretell {
        actor: SeatId,
        source: ObjectId,
        payment: crate::mana::ManaPayment,
    },
    Suspend {
        actor: SeatId,
        source: ObjectId,
        payment: crate::mana::ManaPayment,
    },
    Plot {
        actor: SeatId,
        source: ObjectId,
        payment: crate::mana::ManaPayment,
    },
    TriggerTargetsBound {
        entry: StackEntryId,
    },
    /// Other graveyard cards exiled together as one escape cost.
    EscapePayment {
        entry: StackEntryId,
    },
    /// One selected object contributed to the cost of the identified spell.
    CastLifePayment {
        entry: StackEntryId,
    },
    CastObjectPayment {
        entry: StackEntryId,
        resource: crate::mana::CastResource,
    },
    OpeningShuffle {
        player: SeatId,
        phase: OpeningShufflePhase,
    },
    Draw {
        player: SeatId,
        origin: DrawOrigin,
    },
    OpeningCardsMoved {
        phase: OpeningMovePhase,
        cohort: Vec<SeatId>,
    },
    LandPlay {
        actor: SeatId,
    },
    CastAnnouncement {
        entry: StackEntryId,
    },
    CastCompleted {
        entry: StackEntryId,
    },
    ActivatedAbilityCost {
        entry: StackEntryId,
        ability: ActivatedAbilityId,
    },
    ActivatedAbilityCompleted {
        entry: StackEntryId,
        ability: ActivatedAbilityId,
    },
    ManaAbilityCost {
        source: ObjectId,
        ability: crate::mana::ManaAbilityRef,
    },
    ManaAbilityActivated {
        source: ObjectId,
        ability: crate::mana::ManaAbilityRef,
    },
    ManaProduced {
        source: ObjectId,
        ability: crate::mana::ManaAbilityRef,
    },
    ManaDebited {
        entry: StackEntryId,
    },
    Instruction {
        entry: StackEntryId,
        instruction: usize,
    },
    CounterSpell {
        entry: StackEntryId,
        instruction: usize,
        target: StackEntryId,
    },
    ResolutionCompleted {
        entry: StackEntryId,
        completion: EntryCompletion,
    },
    UntapTurn {
        active: SeatId,
    },
    CleanupDiscard {
        event: CheckEventId,
        boundary: CheckBoundary,
    },
    CleanupCharacteristics {
        position: TurnPosition,
    },
    StateBasedActions {
        event: CheckEventId,
        boundary: CheckBoundary,
    },
    /// Immediate CR 800.4a consequences, following any checked player losses.
    PlayerDeparture,
    DamageAdjusted {
        batch: DamageBatchId,
    },
    DamageDealt {
        batch: DamageBatchId,
    },
    DamageResults {
        batch: DamageBatchId,
    },
    AttackCostPaid {
        position: TurnPosition,
        player: SeatId,
    },
    AttackTaps {
        position: TurnPosition,
        actor: SeatId,
    },
    AttackersDeclared {
        position: TurnPosition,
        actor: SeatId,
    },
    BlockersDeclared {
        position: TurnPosition,
        actor: SeatId,
    },
    CombatDamageAssigned {
        position: TurnPosition,
        actor: SeatId,
        origin: AssignmentOrigin,
    },
    CombatEnded {
        position: TurnPosition,
    },
    ManaPoolsEmptied {
        ended: TurnPosition,
    },
    /// The scheduler passed departed seats before opening the next actual turn.
    TurnOrderAdvanced {
        position: TurnPosition,
    },
    StepBegan {
        position: TurnPosition,
    },
    MechanismUnit,
}
/// One canonical occurrence. Overlapping move properties share its one identity.
// `KeywordActionPerformed` holds `PlayerTriggerAction` inline matching `ObjectFilter` value semantics.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RulesOccurrence {
    /// A scheduled turn of a departed player would have started (CR 800.4k, 800.4m).
    TurnsSkipped {
        players: BTreeSet<SeatId>,
        expired_effects: Vec<ContinuousEffectId>,
    },
    /// A visible permanent left the game without moving to another zone.
    PermanentLeftGame {
        before: Box<SupportedAppearance>,
    },
    /// Result of a fair rules-engine coin flip for the resolving player.
    CoinFlipped {
        player: SeatId,
        won: bool,
    },
    /// A public vote for one exact card during a resolving spell or ability.
    CardVoteCast {
        player: SeatId,
        object: ObjectId,
    },
    GraveyardReturnPaid {
        source: Box<SupportedAppearance>,
        controller: SeatId,
        mana_value: RulesInteger,
    },
    StackExiledToEndTurn {
        entries: Vec<StackEntryId>,
    },
    /// A qualifying discard generated a separately targeted reflexive trigger.
    ReflexiveDiscardDamage {
        source: Box<SupportedAppearance>,
        controller: SeatId,
        amount: RulesInteger,
    },
    /// A generalized CR 603.12 reflexive trigger created during ability/spell resolution.
    ReflexiveTrigger {
        source: Box<SupportedAppearance>,
        controller: SeatId,
        body: Box<ExecutableBody>,
        chosen_x: Option<RulesInteger>,
    },
    CommanderTypeManaSpent {
        source: Box<SupportedAppearance>,
        player: SeatId,
        units: RulesInteger,
    },
    /// Mana added by a resolving effect, not a mana-ability activation.
    ManaAddedByEffect {
        entry: StackEntryId,
        player: SeatId,
        produced: BTreeMap<ManaType, RulesInteger>,
        before: BTreeMap<ManaType, RulesInteger>,
        after: BTreeMap<ManaType, RulesInteger>,
    },
    /// A persistent source-independent emblem was created in the command zone.
    Phased {
        objects: Vec<ObjectId>,
        out: bool,
    },
    Mutated {
        permanent: ObjectId,
    },
    /// A Class permanent reached the indicated level (CR 716).
    ClassLevelReached {
        object: ObjectId,
        level: u8,
    },
    /// A Case permanent became solved (CR 719).
    CaseSolved {
        object: ObjectId,
    },
    /// A permanent became monstrous (CR 701.31).
    BecameMonstrous {
        object: ObjectId,
        count: RulesInteger,
    },
    /// A Room door was unlocked (CR 709.5).
    DoorUnlocked {
        object: ObjectId,
        player: SeatId,
        half: crate::program::SplitHalf,
        fully_unlocked: bool,
    },
    /// A player rolled a die (CR 706).
    DieRolled {
        player: SeatId,
        sides: u16,
        result: u16,
    },
    /// Votes were tallied for a resolving `Instruction::Vote`.
    VotesCast {
        options: Vec<String>,
        counts: Vec<u16>,
    },
    /// A prepared permanent cast a copy of its spell face and became unprepared.
    PreparedSpellCast {
        source: ObjectId,
        entry: StackEntryId,
    },
    AttackDefenderChanged {
        attacker: ObjectId,
        from: crate::targets::TargetRef,
        to: crate::targets::TargetRef,
    },
    EmblemActivated {
        entry: StackEntryId,
        emblem: crate::emblem::EmblemId,
        controller: SeatId,
    },
    EmblemCreated {
        emblem: crate::emblem::Emblem,
    },
    SpeedChanged {
        player: SeatId,
        before: u8,
        after: u8,
    },
    /// A player gained or changed the unique monarch designation.
    MonarchChanged {
        before: Option<SeatId>,
        after: Option<SeatId>,
    },
    CityBlessingGained {
        player: SeatId,
    },
    DayNightChanged {
        before: crate::day_night::DayNight,
        after: crate::day_night::DayNight,
    },
    Transformed {
        before: Box<SupportedAppearance>,
        after: Box<SupportedAppearance>,
    },
    TurnedFaceUp {
        before: Box<SupportedAppearance>,
        after: Box<SupportedAppearance>,
        ability: usize,
    },
    TurnedFaceUpByEffect {
        before: Box<SupportedAppearance>,
        after: Box<SupportedAppearance>,
    },
    FaceDownRevealed {
        object: GameObject,
    },
    /// A card revealed from a looked-at library segment.
    LibraryCardRevealed {
        object: GameObject,
    },
    /// The permanent explored, including when its library was empty.
    Explored {
        source: GameObject,
        controller: SeatId,
    },
    /// A card revealed from hand while choosing how a permanent enters.
    HandCardRevealed {
        object: GameObject,
    },
    /// A privately offered first draw was revealed for its Miracle ability.
    MiracleRevealed {
        object: Box<SupportedAppearance>,
    },
    /// Actual object and opposing controller captured when the target was chosen.
    PermanentTargeted {
        object: ObjectId,
        entry: StackEntryId,
        controller: SeatId,
        instant_or_sorcery: bool,
    },
    /// Actual player and targeting entry captured when a player was chosen as a target.
    PlayerTargeted {
        player: SeatId,
        entry: StackEntryId,
        controller: SeatId,
        spell: bool,
        instant_or_sorcery: bool,
    },
    /// An extra turn was scheduled for this player.
    AdditionalCombatCreated,
    ExtraTurnCreated {
        player: SeatId,
    },
    RegenerationShieldCreated {
        object: ObjectId,
        entry: StackEntryId,
        instruction: usize,
    },
    PermanentRegenerated {
        object: ObjectId,
        entry: StackEntryId,
        instruction: usize,
        removed_damage: RulesInteger,
        removed_from_combat: bool,
    },
    CreaturesGoaded {
        player: SeatId,
        objects: Vec<ObjectId>,
    },
    TurnPreventionCreated {
        creator: StackEntryId,
        instruction: usize,
        controller: SeatId,
        filter: crate::damage::TurnPreventionFilter,
    },
    TurnPreventionSpecCreated {
        creator: StackEntryId,
        instruction: usize,
        controller: SeatId,
        spec: Box<crate::damage::TurnPreventionSpec>,
    },
    /// Damage prevented by a persistent source-independent emblem.
    EmblemDamagePrevented {
        emblem: crate::emblem::EmblemId,
        source: crate::damage::DamageSourceId,
        batch: crate::damage::DamageBatchId,
        stage: u64,
        recipient: crate::damage::DamageRecipient,
        allocations: Vec<crate::damage::DamageAllocation>,
    },
    StaticDamageMultiplied {
        source: ObjectId,
        batch: crate::damage::DamageBatchId,
        stage: u64,
        attempt: crate::damage::DamageAttemptId,
        before: RulesInteger,
        after: RulesInteger,
    },
    DamageReplacedWithMill {
        source: ObjectId,
        batch: crate::damage::DamageBatchId,
        stage: u64,
        player: SeatId,
        allocations: Vec<crate::damage::DamageAllocation>,
    },
    StaticDamagePrevented {
        source: ObjectId,
        batch: crate::damage::DamageBatchId,
        stage: u64,
        recipient: crate::damage::DamageRecipient,
        allocations: Vec<crate::damage::DamageAllocation>,
    },
    TurnDamagePrevented {
        creator: StackEntryId,
        instruction: usize,
        batch: crate::damage::DamageBatchId,
        stage: u64,
        recipient: crate::damage::DamageRecipient,
        allocations: Vec<crate::damage::DamageAllocation>,
    },
    /// One existing stack object acquired the final target plan after a retarget effect.
    SpellControllerChanged {
        entry: StackEntryId,
        before: SeatId,
        after: SeatId,
    },
    StackTargetsChanged {
        entry: StackEntryId,
        before: Vec<TargetChoice>,
        after: Vec<TargetChoice>,
    },
    AbilityCountered {
        entry: StackEntryId,
    },
    /// A creature-powered activation resolved on its source.
    PowerAbilityResolved {
        source: ObjectId,
        kind: crate::activated::PowerAbility,
        contributors: Vec<ObjectId>,
        /// Actual charge counters placed by a station activation, including zero.
        charge_placed: Option<spellwake_contracts::RulesInteger>,
    },
    AttachmentChanged {
        source: ObjectId,
        before: Option<crate::targets::TargetRef>,
        after: Option<crate::targets::TargetRef>,
    },
    LibraryAction(crate::effects::LibraryAction),
    TokenCreated {
        creator: SeatId,
        after: Box<SupportedAppearance>,
    },
    TokenCeased {
        object: GameObject,
    },
    ControllerChanged(ControllerChange),
    LifeChanged(LifeChange),
    CreatureTypeChosen {
        entry: StackEntryId,
        instruction: usize,
        slot: CreatureTypeSlot,
        chooser: SeatId,
        value: spellwake_contracts::creature_types::CreatureTypeId,
    },
    ZoneChange(Box<ZoneOccurrence>),
    StatusChange {
        change: ObjectStatusChange,
        reason: StatusReason,
    },
    Shuffled(ShuffleRecord),
    FailedDraw {
        player: SeatId,
        attempts: RulesInteger,
    },
    CastCompleted(Box<CastOccurrence>),
    ActivatedAbility {
        entry: StackEntryId,
        source: GameObject,
        ability: ActivatedAbilityId,
    },
    ManaAbility {
        source: GameObject,
        ability: crate::mana::ManaAbilityRef,
        controller: SeatId,
    },
    ManaProduced {
        production: ManaProductionRecord,
        before: BTreeMap<ManaType, RulesInteger>,
        after: BTreeMap<ManaType, RulesInteger>,
    },
    /// One positive type row; all rows share the payment's single event/world.
    ManaDebited {
        payment: ManaPaymentRecord,
        mana_type: ManaType,
        amount: RulesInteger,
        before: RulesInteger,
        after: RulesInteger,
    },
    /// Unprotected mana expired while other units remained in the same pool.
    ManaExpired {
        player: SeatId,
        expired: BTreeMap<ManaType, RulesInteger>,
        remaining: BTreeMap<ManaType, RulesInteger>,
    },
    ArtifactTappedForMana {
        source: ObjectId,
        actor: SeatId,
    },
    PermanentTappedForMana {
        source: ObjectId,
        actor: SeatId,
        appearance: Box<SupportedAppearance>,
    },
    CardDrawOrdinal {
        player: SeatId,
        number: RulesInteger,
    },
    ManaPoolEmptied {
        player: SeatId,
        previous: BTreeMap<ManaType, RulesInteger>,
    },
    AbilityCopied {
        entry: StackEntryId,
        original: StackEntryId,
        controller: SeatId,
        targets: Vec<TargetChoice>,
    },
    CopyCreated {
        copiable_keywords: BTreeSet<KeywordAbility>,
        copiable_card_types: BTreeSet<CardType>,
        copiable_creature_subtypes: BTreeSet<CreatureTypeId>,
        copiable_spell_subtypes: BTreeSet<crate::program::SpellSubtype>,
        copiable_artifact_subtypes: BTreeSet<crate::program::ArtifactSubtype>,
        copiable_enchantment_subtypes: BTreeSet<crate::program::EnchantmentSubtype>,
        copiable_names: spellwake_contracts::names::ObjectNames,
        copiable_supertypes: BTreeSet<crate::program::Supertype>,
        copiable_triggered: Vec<TriggeredAbility>,
        entry: StackEntryId,
        original: StackEntryId,
        controller: SeatId,
        targets: Vec<TargetChoice>,
    },
    /// Actual uninterrupted World membership change; never a copiable timestamp.
    WorldTenureChanged(crate::characteristics::WorldTenureChange),
    CopyMoved {
        before: SpellCopyObject,
        after: SpellCopyObject,
    },
    CopyCeased {
        copy: SpellCopyObject,
    },
    /// A resolved player acquires lasting protection from this chosen color.
    PlayerProtectionGranted {
        player: SeatId,
        color: crate::characteristics::Color,
    },
    ContinuousEffectCreated {
        effect: ResolvedContinuousEffect,
        appearances: Vec<CharacteristicAppearanceChange>,
    },
    /// Source-dependent continuous effects ended before the next game action.
    SourceControlEffectsEnded {
        effects: Vec<ContinuousEffectId>,
    },
    CleanupCharacteristics {
        expired_effects: Vec<ContinuousEffectId>,
        marks_removed: Vec<MarkedDamageRemoval>,
        appearances: Vec<CharacteristicAppearanceChange>,
    },
    CombatantsRemoved {
        participants: Vec<Combatant>,
    },
    TimeCounterRemoved {
        object: ObjectId,
        before: RulesInteger,
        after: RulesInteger,
    },
    FadeCounterRemoved {
        object: ObjectId,
        before: RulesInteger,
        after: RulesInteger,
    },
    AgeCounterPlaced {
        object: ObjectId,
        before: RulesInteger,
        after: RulesInteger,
    },
    /// Physical loyalty counters changed while paying an activated ability cost.
    PermanentCountersDoubled {
        selected: BTreeMap<crate::targets::TargetRef, BTreeSet<crate::effects::CounterKind>>,
    },
    Proliferated {
        player: SeatId,
        selected: BTreeMap<crate::targets::TargetRef, BTreeSet<crate::effects::CounterKind>>,
    },
    LoreCountersChanged {
        object: ObjectId,
        before: RulesInteger,
        after: RulesInteger,
        exact_chapter_only: bool,
    },
    /// A state-trigger condition became eligible and has no pending instance (CR 603.8).
    CounterThresholdMet {
        object: ObjectId,
        definition: TriggeredAbilityId,
        grant: Option<crate::characteristics::EffectInstanceKey>,
        kind: crate::effects::CounterKind,
    },
    SubtypeThresholdMet {
        object: ObjectId,
        definition: TriggeredAbilityId,
        grant: Option<crate::characteristics::EffectInstanceKey>,
    },
    /// A general world-evaluable state-trigger condition became true (CR 603.8).
    StateConditionMet {
        object: ObjectId,
        definition: TriggeredAbilityId,
        grant: Option<crate::characteristics::EffectInstanceKey>,
    },
    /// A player performed a tracked keyword action (for example collect evidence, exert, investigate, connive).
    KeywordActionPerformed {
        player: SeatId,
        action: crate::program::PlayerTriggerAction,
    },
    PermanentCounterChanged {
        object: ObjectId,
        kind: crate::effects::CounterKind,
        before: RulesInteger,
        after: RulesInteger,
    },
    SuspectCountersChanged {
        object: ObjectId,
        before: RulesInteger,
        after: RulesInteger,
    },
    ChargeCountersChanged {
        object: ObjectId,
        before: RulesInteger,
        after: RulesInteger,
    },
    StorageCountersChanged {
        object: ObjectId,
        before: RulesInteger,
        after: RulesInteger,
    },
    LoyaltyCountersChanged {
        object: ObjectId,
        before: RulesInteger,
        after: RulesInteger,
    },
    CountersPlaced {
        object: ObjectId,
        before: PowerToughnessCounters,
        added: PowerToughnessCounters,
        after: PowerToughnessCounters,
        before_appearance: Box<SupportedAppearance>,
        after_appearance: Box<SupportedAppearance>,
    },
    /// Counters removed by a resolving ordinary effect.
    PowerToughnessCountersRemoved {
        object: ObjectId,
        before: PowerToughnessCounters,
        after: PowerToughnessCounters,
    },
    CounterRemovedForGraft {
        object: ObjectId,
        before: PowerToughnessCounters,
        after: PowerToughnessCounters,
    },
    CountersCancelled {
        object: ObjectId,
        before: PowerToughnessCounters,
        removed: PowerToughnessCounters,
        after: PowerToughnessCounters,
    },
    DamageEffectCreated(DamageEffectCreation),
    DamagePrevented {
        application: PreventionApplication,
        damage_before: BTreeMap<DamageAttemptId, RulesInteger>,
        shield_before: RulesInteger,
    },
    ProtectionPrevented(crate::damage::ProtectionPrevention),
    DamageMultiplied(DamageMultiplication),
    DamageDealt {
        damage: AppliedDamage,
        source_subtypes: BTreeSet<CreatureTypeId>,
        kind: DamageKind,
        origin: DamageOrigin,
    },
    DamageResult {
        recipient: DamageRecipient,
        before: RulesInteger,
        after: RulesInteger,
        components: Vec<DamageAttemptId>,
    },
    CreatureEvolved {
        object: ObjectId,
    },
    CreatureMentored {
        source: ObjectId,
        target: ObjectId,
    },
    BecameRenowned {
        object: ObjectId,
    },
    ExperienceCountersChanged {
        player: SeatId,
        before: RulesInteger,
        after: RulesInteger,
    },
    PlayerCounterChanged {
        player: SeatId,
        kind: crate::effects::CounterKind,
        before: RulesInteger,
        after: RulesInteger,
    },
    PlayerPoisoned {
        player: SeatId,
        before: RulesInteger,
        after: RulesInteger,
        contributions: Vec<PoisonContribution>,
    },
    PlayerLost {
        player: SeatId,
        reasons: BTreeSet<PlayerLossReason>,
    },
    AttackCostPaid {
        player: SeatId,
        cost: crate::combat::AttackCost,
        white_symbols_paid_with_life: RulesInteger,
        mana: Vec<crate::mana::ManaSpend>,
    },
    AttackersDeclared {
        origin: DeclarationOrigin,
        selected: Vec<AttackDeclaration>,
        membership: Vec<AttackingCreature>,
    },
    BlockersDeclared {
        origin: DeclarationOrigin,
        selected: Vec<BlockDeclaration>,
        membership: Vec<BlockingCreature>,
        statuses: Vec<(ObjectId, BlockStatus, BlockStatus)>,
    },
    CombatDamageAssigned(CombatDamageAssignment),
    CombatEnded {
        previous: Option<CombatState>,
    },
    FinalSagaChapterCompleted {
        saga: ObjectId,
        controller: SeatId,
        mana_value: RulesInteger,
    },
    EntryCompleted {
        entry: StackEntryId,
        controller: SeatId,
        kind: StackKind,
        completion: EntryCompletion,
    },
    StepBegan {
        position: TurnPosition,
        active: SeatId,
    },
    /// Two creatures fought each other (CR 701.12).
    PermanentFought {
        object: ObjectId,
        opponent: ObjectId,
    },
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EventOccurrence {
    pub id: OccurrenceId,
    pub value: RulesOccurrence,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RulesEvent {
    pub id: EventId,
    pub cause: EventCause,
    pub occurrences: Vec<EventOccurrence>,
}
impl RulesOccurrence {
    /// Previous battlefield appearance for effects that need a departed permanent's information.
    pub(crate) fn departing_permanent(&self) -> Option<&SupportedAppearance> {
        match self {
            Self::PermanentLeftGame { before } => Some(before),
            Self::ZoneChange(change)
                if change.before.object.zone() == ObjectZone::Battlefield
                    && change.after.object.zone() != ObjectZone::Battlefield =>
            {
                Some(&change.before)
            }
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WaitingInstance {
    pub id: TriggerInstanceId,
    /// Printed definition ID or zero-based occurrence within this grant.
    pub definition: TriggeredAbilityId,
    /// Together with the source and definition ID, identifies a granted ability instance.
    pub granting_effect: Option<crate::characteristics::EffectInstanceKey>,
    pub causes: TriggerCauseSet,
    pub source: SupportedAppearance,
    pub controller: SeatId,
    pub body: ExecutableBody,
}
/// Execution keeps no duplicate controller; the real StackEntry controls the body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TriggerBinding {
    pub instance: TriggerInstanceId,
    /// Printed definition ID or zero-based occurrence within this grant.
    pub definition: TriggeredAbilityId,
    /// Together with the source and definition ID, identifies a granted ability instance.
    pub granting_effect: Option<crate::characteristics::EffectInstanceKey>,
    pub causes: TriggerCauseSet,
    pub source: SupportedAppearance,
}
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ExecutionBindings {
    pub trigger: Option<TriggerBinding>,
}
impl WaitingInstance {
    pub(crate) fn into_payload_parts(self) -> (ExecutableBody, TriggerBinding) {
        let binding = TriggerBinding {
            instance: self.id,
            definition: self.definition,
            granting_effect: self.granting_effect,
            causes: self.causes,
            source: self.source,
        };
        (self.body, binding)
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TriggerPlacement {
    pub instance: TriggerInstanceId,
    pub wave: PlacementWaveId,
    pub controller: SeatId,
    pub position: usize,
    pub entry: StackEntryId,
}
/// Required placement-time target selection, before priority can be granted.
/// Its private fields describe one exact pending native revision, not host authority.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TriggerTargetChoice {
    pub(crate) modal: Option<TriggerModesChoice>,
    pub(crate) secondary_targets:
        Option<BTreeMap<crate::targets::TargetRef, Vec<crate::targets::TargetRef>>>,
    pub(crate) bounds: Option<(usize, usize)>,
    pub(crate) ordered_any: bool,
    pub(crate) divides_three_damage: bool,
    pub(crate) revision: u64,
    pub(crate) boundary: CheckBoundary,
    pub(crate) event: CheckEventId,
    pub(crate) wave: PlacementWaveId,
    pub(crate) chooser: SeatId,
    pub(crate) instance: TriggerInstanceId,
    pub(crate) entry: StackEntryId,
    pub(crate) domain: Vec<crate::targets::TargetRef>,
}
/// Available printed modes and the exact current domain of each mode's target.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TriggerModesChoice {
    pub minimum: u8,
    pub maximum: u8,
    pub distinct_targets: bool,
    pub allow_repeat: bool,
    pub modes: Vec<TriggerModeOption>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TriggerModeOption {
    pub index: u8,
    pub multiple: Option<(usize, usize)>,
    pub target_required: bool,
    pub targets: Vec<crate::targets::TargetRef>,
}

impl TriggerTargetChoice {
    pub fn modal(&self) -> Option<&TriggerModesChoice> {
        self.modal.as_ref()
    }
    /// Legal optional creature targets grouped by the chosen first target.
    pub fn secondary_targets(
        &self,
    ) -> Option<&BTreeMap<crate::targets::TargetRef, Vec<crate::targets::TargetRef>>> {
        self.secondary_targets.as_ref()
    }
    /// Multiple objects picked for the same printed occurrence of "target".
    pub fn permanent_target_bounds(&self) -> Option<(usize, usize)> {
        if self.ordered_any { None } else { self.bounds }
    }
    /// Ordered choice of any-target recipients, with the first chosen separately meaningful.
    pub fn ordered_any_target_bounds(&self) -> Option<(usize, usize)> {
        if self.ordered_any { self.bounds } else { None }
    }
    /// Divide three damage among the ordered recipients, assigning any remainder to the first.
    pub fn divides_three_damage(&self) -> bool {
        self.divides_three_damage
    }

    /// Malformed-frame probe with no change to the owning wave allocator.
    #[cfg(test)]
    pub(crate) fn qualification_with_next_wave(&self) -> Result<Self, TriggerError> {
        let wave = self
            .wave
            .0
            .checked_add(1)
            .ok_or(TriggerError::WaveIdentityExhausted)?;
        Ok(Self {
            wave: PlacementWaveId(wave),
            ..self.clone()
        })
    }
    /// Executor revision at which this view was offered.
    pub fn revision(&self) -> u64 {
        self.revision
    }
    /// Actual checks boundary; placement occurs only under Priority checks.
    pub fn boundary(&self) -> CheckBoundary {
        self.boundary
    }
    /// The current state-check iteration.
    pub fn event(&self) -> CheckEventId {
        self.event
    }
    /// Frozen APNAP placement wave.
    pub fn wave(&self) -> PlacementWaveId {
        self.wave
    }
    /// Captured controller responsible for this choice.
    pub fn chooser(&self) -> SeatId {
        self.chooser
    }
    /// Waiting instance that still owns the captured body and causes.
    pub fn instance(&self) -> TriggerInstanceId {
        self.instance
    }
    /// Actual provisional stack entry, without an installed body yet.
    pub fn entry(&self) -> StackEntryId {
        self.entry
    }
    /// Nonempty unique legal domain in native TargetRef order.
    pub fn domain(&self) -> &[crate::targets::TargetRef] {
        &self.domain
    }
}
/// Placement removal is separate from later resolution target failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TriggerRemovalReason {
    /// No legal choice exists for a required single target (CR 603.3d).
    NoLegalTargets,
}
/// Exact current ordinary cohort domain; the reply chooses bottom-to-top order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TriggerOrder {
    pub(crate) revision: u64,
    pub(crate) boundary: CheckBoundary,
    pub(crate) event: CheckEventId,
    pub(crate) wave: PlacementWaveId,
    pub(crate) chooser: SeatId,
    pub(crate) instances: Vec<TriggerInstanceId>,
}
impl TriggerOrder {
    /// Explicit malformed-frame probe, unavailable in production builds.
    /// This clones only the offered frame and never touches the owner's allocator.
    #[cfg(test)]
    pub(crate) fn qualification_with_next_wave(&self) -> Result<Self, TriggerError> {
        let wave = self
            .wave
            .0
            .checked_add(1)
            .ok_or(TriggerError::WaveIdentityExhausted)?;
        Ok(Self {
            wave: PlacementWaveId(wave),
            ..self.clone()
        })
    }
    pub fn revision(&self) -> u64 {
        self.revision
    }
    pub fn boundary(&self) -> CheckBoundary {
        self.boundary
    }
    pub fn event(&self) -> CheckEventId {
        self.event
    }
    pub fn wave(&self) -> PlacementWaveId {
        self.wave
    }
    pub fn chooser(&self) -> SeatId {
        self.chooser
    }
    pub fn instances(&self) -> &[TriggerInstanceId] {
        &self.instances
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RulesRecord {
    /// Declaring attackers was reversed before the attack cost was paid.
    AttackDeclarationReversed {
        player: SeatId,
        attackers: Vec<ObjectId>,
        kept_mana_abilities: bool,
        reversed_mana: BTreeSet<crate::game::AttackManaId>,
        retracted_tap_triggers: BTreeSet<TriggerInstanceId>,
    },
    /// An unfinished child cast was reversed after irreversible mana effects.
    CastAnnouncementReversed {
        entry: StackEntryId,
        stack_object: ObjectId,
        restored: ObjectId,
    },
    /// A source-independent emblem triggered from one actual spell cast.
    EmblemTriggerDiscovered {
        instance: TriggerInstanceId,
        controller: SeatId,
        emblem: crate::emblem::EmblemId,
        cause: OccurrenceId,
    },
    /// A source-free inherent monarch trigger, controlled by the former monarch.
    MonarchTriggerDiscovered {
        instance: TriggerInstanceId,
        controller: SeatId,
        beneficiary: Option<SeatId>,
        causes: Vec<OccurrenceId>,
    },
    /// A source-independent delayed trigger from an already resolved spell copy.
    DelayedCopyTriggerDiscovered {
        instance: TriggerInstanceId,
        controller: SeatId,
        original: StackEntryId,
        cause: OccurrenceId,
    },
    /// A player's inherent speed trigger has no object source (CR 718).
    SpeedTriggerDiscovered {
        instance: TriggerInstanceId,
        controller: SeatId,
        causes: Vec<OccurrenceId>,
    },
    /// Root-owned immediate end; not a triggerable event or child allocation.
    GameConcluded {
        conclusion: crate::game::GameConclusion,
    },
    /// Resolution-time choice, independent of trigger discovery and placement.
    OptionalEffectOutcome {
        entry: StackEntryId,
        instruction: usize,
        chooser: SeatId,
        outcome: crate::effects::OptionalEffectChoice,
        next: usize,
    },
    /// The resolving instruction's original chooser has been replaced under CR 800.4g.
    ChooserReplaced {
        entry: StackEntryId,
        instruction: usize,
        departed: SeatId,
        chooser: SeatId,
        selected: SeatId,
    },
    /// Actual quoted resolution cost and explicit result; not a Magic event.
    ResolutionPaymentOutcome {
        entry: StackEntryId,
        instruction: usize,
        payer: SeatId,
        cost: crate::mana::ManaCost,
        restricted_colors: Option<[crate::mana::ManaType; 2]>,
        outcome: crate::effects::ResolutionPaymentOutcome,
        next: usize,
    },
    /// Actual conditional instruction evaluation; not a Magic event.
    InstructionBranch {
        entry: StackEntryId,
        instruction: usize,
        condition: crate::program::ResolutionCondition,
        left: RulesInteger,
        right: RulesInteger,
        result: bool,
        next: usize,
    },
    /// Actual forward jump; does not publish a triggerable occurrence.
    InstructionJump {
        entry: StackEntryId,
        instruction: usize,
        next: usize,
    },
    Event(RulesEvent),
    TriggerDiscovered(Box<WaitingInstance>),
    TriggerPlacementStarted(TriggerPlacement),
    TriggerTargetSelected {
        placement: TriggerPlacement,
        target: crate::targets::TargetChoice,
    },
    TriggerRemoved {
        placement: TriggerPlacement,
        reason: TriggerRemovalReason,
    },
    TriggerPlaced(TriggerPlacement),
}
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RulesJournal {
    pub records: Vec<RulesRecord>,
}
impl RulesJournal {
    pub fn append(&mut self, mut other: Self) {
        self.records.append(&mut other.records);
    }
}
/// A successful local transition always transports its current append with its state/result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RulesTransition<S, R> {
    state: Box<S>,
    result: R,
    append: RulesJournal,
}
impl<S, R> RulesTransition<S, R> {
    pub(crate) fn new(state: S, result: R, append: RulesJournal) -> Self {
        Self {
            state: Box::new(state),
            result,
            append,
        }
    }
    pub fn state(&self) -> &S {
        &self.state
    }
    pub fn result(&self) -> &R {
        &self.result
    }
    pub fn append(&self) -> &RulesJournal {
        &self.append
    }
    pub fn into_parts(self) -> (S, R, RulesJournal) {
        (*self.state, self.result, self.append)
    }
    pub(crate) fn into_boxed_parts(self) -> (Box<S>, R, RulesJournal) {
        (self.state, self.result, self.append)
    }
    /// Change a typed result without dropping the successor or its current append.
    pub fn map_result<T>(self, map: impl FnOnce(R) -> T) -> RulesTransition<S, T> {
        RulesTransition {
            state: self.state,
            result: map(self.result),
            append: self.append,
        }
    }
    /// Explicitly discard the operation result and append outside receipt-preserving paths.
    pub fn into_state(self) -> S {
        *self.state
    }
}

/// Complete relevant immutable queries at one actual cut, not a replayed journal world.
pub(crate) struct RulesView<'a> {
    pub(crate) morbid: bool,
    pub(crate) previous_turn_casts: Option<crate::program::PreviousTurnCastCondition>,
    pub(crate) window: &'a StackWindow,
    pub(crate) cards: &'a GameObjects,
    pub(crate) name_rules: &'a spellwake_contracts::names::NameRules,
    pub(crate) programs: &'a BTreeMap<crate::object_identity::CardId, crate::program::Program>,
    pub(crate) characteristics: &'a CharacteristicState,
    pub(crate) evaluated: Arc<EvaluatedCharacteristics>,
    pub(crate) marks: &'a BTreeMap<ObjectId, RulesInteger>,
    pub(crate) mana: &'a BTreeMap<(SeatId, ManaType), RulesInteger>,
    pub(crate) life: &'a LifeState,
    pub(crate) combat: Option<&'a CombatState>,
    pub(crate) step: Option<crate::turn::Step>,
    pub(crate) turn: Option<&'a TurnScheduler>,
}
impl RulesView<'_> {
    pub(crate) fn opponent_lost_life(&self, controller: SeatId, minimum: u8) -> bool {
        self.characteristics
            .life_lost_this_turn
            .iter()
            .any(|(&seat, amount)| {
                player_matches(PlayerRelation::Opponent, controller, seat, self)
                    && amount >= &i64::from(minimum).into()
            })
    }
    pub(crate) fn controls_own_commander(&self, controller: SeatId) -> bool {
        self.evaluated.objects.values().any(|view| {
            view.zone == ObjectZone::Battlefield
                && view.commander_owner == Some(controller)
                && view.controller == Some(controller)
        })
    }
    /// Whether a controller has an artifact tied for the greatest current mana value.
    pub(crate) fn controls_greatest_artifact(&self, controller: SeatId) -> bool {
        let mut greatest: Option<RulesInteger> = None;
        let mut greatest_owned: Option<RulesInteger> = None;
        for object in self.evaluated.objects.values().filter(|object| {
            object.zone == ObjectZone::Battlefield
                && object.has_type(crate::program::CardType::Artifact)
        }) {
            let Some(physical) = self.cards.get(object.object) else {
                return false;
            };
            let value = if physical.face_down().is_some() {
                Some(0.into())
            } else if physical.transformed()
                && self
                    .characteristics
                    .copied_definition(object.object)
                    .is_none()
            {
                physical
                    .card_id()
                    .and_then(|id| self.programs.get(&id))
                    .map(crate::program::Program::mana_value)
            } else {
                self.characteristics
                    .copiable_definition(physical, self.programs)
                    .map(|(program, _)| program.mana_value())
            };
            let Some(value) = value else {
                return false;
            };
            if greatest.as_ref().is_none_or(|old| &value > old) {
                greatest = Some(value.clone());
            }
            if object.controller == Some(controller)
                && greatest_owned.as_ref().is_none_or(|old| &value > old)
            {
                greatest_owned = Some(value);
            }
        }
        greatest.is_some() && greatest == greatest_owned
    }
    fn land_count(&self, seat: SeatId) -> usize {
        self.evaluated
            .objects
            .values()
            .filter(|object| {
                object.zone == ObjectZone::Battlefield
                    && object.controller == Some(seat)
                    && object.has_type(crate::program::CardType::Land)
            })
            .count()
    }
    pub(crate) fn opponent_has_more_lands(&self, controller: SeatId) -> bool {
        self.players_with_more_lands(controller) > 0
    }
    pub(crate) fn players_with_more_lands(&self, controller: SeatId) -> usize {
        let own = self.land_count(controller);
        self.window
            .participants()
            .iter()
            .copied()
            .filter(|&seat| seat != controller && self.land_count(seat) > own)
            .count()
    }
    /// Capture only the canonical physical spell in this actual completed world.
    ///
    /// # Errors
    /// Returns `InvalidWorld` for a missing/noncard entry, inconsistent identities
    /// or controllers, or unavailable evaluated physical Stack appearance.
    pub(crate) fn cast_occurrence(
        &self,
        entry: StackEntryId,
        controller: SeatId,
        spend: &[crate::mana::ManaSpend],
        mana_value: RulesInteger,
    ) -> Result<CastOccurrence, TriggerError> {
        let actual = self
            .window
            .entries()
            .iter()
            .find(|value| value.id() == entry)
            .ok_or(TriggerError::InvalidWorld)?;
        if actual.controller() != controller || !self.window.participants().contains(&controller) {
            return Err(TriggerError::InvalidWorld);
        }
        let StackKind::CardSpell(object) = actual.kind() else {
            return Err(TriggerError::InvalidWorld);
        };
        let current = self
            .evaluated
            .get(*object)
            .ok_or(TriggerError::InvalidWorld)?;
        let spell = self.appearance(*object).ok_or(TriggerError::InvalidWorld)?;
        if spell.object.zone() != ObjectZone::Stack
            || spell.object.controller() != Some(controller)
            || spell.object.card_id().is_none()
            || current.object != *object
            || current.controller != Some(controller)
            || current.abilities.source != *object
            || current.abilities.card != spell.object.card_id()
        {
            return Err(TriggerError::InvalidWorld);
        }
        let increment = self.evaluated.objects.values().any(|current| {
            current.zone == ObjectZone::Battlefield
                && current.controller == Some(controller)
                && current.printed_text.triggered.iter().any(|ability| {
                    ability.condition.event == TriggerEvent::Keyword(TriggerKeywordEvent::Increment)
                        && current.abilities.triggered.contains(&ability.id)
                })
        });
        Ok(CastOccurrence {
            cast_from_library: actual.cast_from_library,
            cast_from_graveyard: actual.cast_from_graveyard,
            cast_from_exile: actual.cast_summary.cast_from_exile,
            cast_from_hand: actual.cast_from_any_hand(),
            cast_original_object: actual.cast_original_object,
            ordinal_this_turn: None,
            mana_value,
            copy_source: None,
            spent_for_increment: increment.then(|| {
                spend
                    .iter()
                    .fold(RulesInteger::from(0), |sum, mana| sum.plus(&mana.amount))
            }),
            total_mana_spent: actual.cast_summary.total_mana.clone(),
            entry,
            controller,
            spell,
        })
    }
    pub(crate) fn mana_pool(&self, player: SeatId) -> Option<BTreeMap<ManaType, RulesInteger>> {
        self.window.participants().contains(&player).then(|| {
            ManaType::ALL
                .into_iter()
                .map(|kind| {
                    (
                        kind,
                        self.mana
                            .get(&(player, kind))
                            .cloned()
                            .unwrap_or_else(|| 0.into()),
                    )
                })
                .collect()
        })
    }
    pub(crate) fn appearance(&self, object: ObjectId) -> Option<SupportedAppearance> {
        let physical = self.cards.get(object)?;
        let current = self.evaluated.get(object)?;
        let battlefield = physical.zone() == ObjectZone::Battlefield;
        let stack_spell = (physical.zone() == ObjectZone::Stack)
            .then(|| {
                self.window
                    .entries()
                    .iter()
                    .chain(self.window.resolving())
                    .find(|entry| self.window.spell_object(entry) == Some(object))
            })
            .flatten();
        Some(SupportedAppearance {
            cast_origin: stack_spell.and_then(crate::stack_window::StackEntry::cast_origin),
            spell_x: stack_spell
                .and_then(crate::stack_window::StackEntry::chosen_x)
                .cloned(),
            kicker_choices: stack_spell
                .map(|entry| entry.kicker_choices().clone())
                .or_else(|| {
                    self.characteristics
                        .kicker_counts
                        .get(&object)
                        .cloned()
                        .filter(|counts| !counts.is_empty())
                })
                .unwrap_or_else(|| {
                    if self.characteristics.kicked.contains(&object) {
                        BTreeMap::from([(0, 1.into())])
                    } else {
                        BTreeMap::new()
                    }
                }),
            commander_owner: current.commander_owner,
            object: physical.clone(),
            copiable_names: current.defined.names.clone(),
            current_names: current.current_names.clone(),
            copiable_supertypes: current.defined.supertypes.clone(),
            current_supertypes: current.current_supertypes.clone(),
            copiable_card_types: current.defined.types.clone(),
            copiable_creature_subtypes: current.defined.creature_subtypes.clone(),
            copiable_land_subtypes: current.defined.land_subtypes.clone(),
            copiable_spell_subtypes: current.defined.spell_subtypes.clone(),
            copiable_artifact_subtypes: current.defined.artifact_subtypes.clone(),
            copiable_enchantment_subtypes: current.defined.enchantment_subtypes.clone(),
            copiable_planeswalker_subtypes: current.defined.planeswalker_subtypes.clone(),
            types: current.current_types.clone(),
            creature_subtypes: current.current_creature_subtypes.clone(),
            land_subtypes: current.current_land_subtypes.clone(),
            spell_subtypes: current.current_spell_subtypes.clone(),
            artifact_subtypes: current.current_artifact_subtypes.clone(),
            enchantment_subtypes: current.current_enchantment_subtypes.clone(),
            planeswalker_subtypes: current.current_planeswalker_subtypes.clone(),
            intrinsic_mana: current.intrinsic_mana.clone(),
            colors: current.current_colors.clone(),
            abilities: current.abilities.clone(),
            printed_text: current.printed_text.clone(),
            saddled: current.saddled,
            attached_to: current.attached_to,
            keywords: current.keywords.clone(),
            time_counters: self.characteristics.time_counters.get(&object).cloned(),
            fade_counters: self.characteristics.fade.get(&object).cloned(),
            previous_upkeep: current
                .printed_text
                .triggered
                .iter()
                .any(|a| {
                    a.condition.event
                        == TriggerEvent::Keyword(TriggerKeywordEvent::Upkeep(
                            crate::program::UpkeepKeyword::Echo,
                        ))
                })
                .then(|| {
                    self.characteristics
                        .upkeep_cuts
                        .as_ref()?
                        .get(&physical.controller()?)
                        .copied()
                })
                .flatten(),
            renowned: self.characteristics.renowned.contains(&object),
            monstrous: self.characteristics.monstrous.contains(&object),
            suspected: self
                .characteristics
                .suspect_counters
                .get(&object)
                .is_some_and(RulesInteger::is_positive),
            goaded: false,
            solved: self.characteristics.solved_cases.contains(&object),
            prepared: self.characteristics.prepared.contains(&object),
            exerted: self
                .characteristics
                .exerted_objects_this_turn
                .contains(&object),
            has_adventure: current.defined.has_adventure,
            has_x_in_cost: current.defined.has_x_in_cost,
            has_toxic: current
                .toxic_value
                .as_ref()
                .is_some_and(crate::RulesInteger::is_positive),
            has_no_abilities: current.has_no_abilities(),
            toxic_value: current.toxic_value.clone(),
            granted_triggers: current.granted_triggers.clone(),
            granted_unblockable: current.granted_unblockable,
            granted_exalted: current.granted_exalted.clone(),
            granted_combat_keywords: current.granted_combat_keywords.clone(),
            copiable_power_toughness: current.defined.printed_pt.clone(),
            battlefield_total_counters: battlefield
                .then(|| self.characteristics.total_permanent_counters(object)),
            battlefield_counters: battlefield.then(|| self.characteristics.counters(object)),
            battlefield_marked_damage: battlefield
                .then(|| self.marks.get(&object).cloned().unwrap_or_else(|| 0.into())),
            current_power_toughness: current
                .power_toughness
                .as_ref()
                .map(|pt| pt.derived.clone()),
        })
    }
}

/// Pure event shape admission, before any identity allocation or world query.
pub(crate) fn validate_event_shape(values: &[RulesOccurrence]) -> Result<(), TriggerError> {
    if values.is_empty() {
        Err(TriggerError::EmptyEvent)
    } else {
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum DelayedStepAction {
    ReturnExiledWithCounters(Vec<ObjectId>),
    Exile {
        step: crate::turn::Step,
        objects: Vec<ObjectId>,
    },
    Sacrifice {
        step: crate::turn::Step,
        objects: Vec<ObjectId>,
    },
    Cast {
        object: ObjectId,
        kind: crate::program::EndStepCast,
    },
    ReturnFromExile(Vec<ObjectId>),
    ReturnFromGraveyard {
        object: ObjectId,
        while_source_remains: bool,
    },
    ReturnFromGraveyardTransformed(ObjectId),
    ReturnGraveyardToHand(ObjectId),
    ReturnPermanentToHand(ObjectId),
    PhaseOut(Vec<ObjectId>),
    Unearth(ObjectId),
    Rebound(ObjectId),
    /// Generalized CR 603.7 delayed step trigger executing an authored body.
    ExecuteBody {
        step: crate::turn::Step,
        own_step_only: bool,
        body: Box<ExecutableBody>,
    },
}
#[derive(Debug, Clone, PartialEq, Eq)]
struct DelayedStepTrigger {
    source: SupportedAppearance,
    controller: SeatId,
    action: DelayedStepAction,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct WaitingEmblem {
    pub controller: SeatId,
    pub emblem: crate::emblem::EmblemId,
    pub body: ExecutableBody,
    pub trigger: Option<TriggerBinding>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AbilityCopyWatch {
    NextLoyalty,
    PlaneswalkerType(crate::program::PlaneswalkerSubtype),
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum DelayedCopyAction {
    Draw(RulesInteger, BTreeSet<crate::characteristics::Color>),
    ReturnExiledWithCounters(Vec<ObjectId>),
    ReturnExiled(Vec<ObjectId>),
    Sacrifice(Vec<ObjectId>),
    Exile(Vec<ObjectId>),
    SpellTwice(Box<CastOccurrence>, BTreeSet<crate::characteristics::Color>),
    AbilityTwice(
        Arc<crate::effects::CapturedAbilityCopy>,
        BTreeSet<crate::characteristics::Color>,
    ),
    GrantCastCardAbilities {
        recipient: ObjectId,
        donor: Box<crate::characteristics::ObjectCharacteristics>,
    },
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct GraveyardCastWatch {
    player: SeatId,
    original: StackEntryId,
    recipient: ObjectId,
    card: ObjectId,
}

/// Only checked counters and owned waiting instances survive acceptance.
/// Event-driven delayed triggered ability (CR 603.7) scheduled by `Instruction::ScheduleDelayedEventTrigger`.
#[derive(Debug, Clone, PartialEq, Eq)]
struct DelayedEventTrigger {
    source: SupportedAppearance,
    controller: SeatId,
    trigger: crate::program::TriggerSpec,
    body: ExecutableBody,
    expires: crate::characteristics::ResolvedDuration,
    once: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TriggerState {
    next_event: u64,
    next_instance: u64,
    watched_graveyard_casts: Vec<GraveyardCastWatch>,
    next_wave: u64,
    waiting: BTreeMap<TriggerInstanceId, WaitingInstance>,
    speed_waiting: BTreeMap<TriggerInstanceId, SeatId>,
    monarch_waiting: BTreeMap<TriggerInstanceId, (SeatId, Option<SeatId>)>,
    emblem_waiting: BTreeMap<TriggerInstanceId, WaitingEmblem>,
    combat_draws: Vec<combat_draw::CombatDrawWatch>,
    weakening_attack_triggers: Vec<(SupportedAppearance, SeatId, BTreeSet<SeatId>)>,
    delayed_emblem_returns: Vec<(crate::emblem::EmblemId, SeatId, ObjectId)>,
    delayed_copy_waiting: BTreeMap<TriggerInstanceId, (SeatId, StackEntryId, DelayedCopyAction)>,
    delayed_copy_steps: Vec<(SeatId, StackEntryId, crate::turn::Step, DelayedCopyAction)>,
    token_leave_triggers: BTreeMap<ObjectId, (SupportedAppearance, SeatId)>,
    earthbent_returns: BTreeMap<ObjectId, (SupportedAppearance, SeatId)>,
    speed_triggered_turn: Option<(u64, SeatId)>,
    current_speed_turn: Option<(u64, SeatId)>,
    declared_attackers_this_turn: BTreeSet<ObjectId>,
    once_this_turn: BTreeSet<(
        ObjectId,
        Option<crate::characteristics::EffectInstanceKey>,
        TriggeredAbilityId,
    )>,
    used_optional_trigger_effects: BTreeSet<(
        ObjectId,
        Option<crate::characteristics::EffectInstanceKey>,
        TriggeredAbilityId,
        SeatId,
    )>,
    delayed_steps: Vec<DelayedStepTrigger>,
    delayed_event_triggers: Vec<DelayedEventTrigger>,
    next_spell_or_loyalty: Vec<(
        SeatId,
        StackEntryId,
        BTreeSet<crate::characteristics::Color>,
    )>,
    ability_copy_watches: Vec<(SupportedAppearance, SeatId, AbilityCopyWatch)>,
}
impl Default for TriggerState {
    fn default() -> Self {
        Self {
            next_event: 1,
            next_instance: 1,
            watched_graveyard_casts: Vec::new(),
            next_wave: 1,
            waiting: BTreeMap::new(),
            speed_waiting: BTreeMap::new(),
            monarch_waiting: BTreeMap::new(),
            emblem_waiting: BTreeMap::new(),
            delayed_emblem_returns: Vec::new(),
            combat_draws: Vec::new(),
            weakening_attack_triggers: Vec::new(),
            delayed_copy_waiting: BTreeMap::new(),
            delayed_copy_steps: Vec::new(),
            token_leave_triggers: BTreeMap::new(),
            earthbent_returns: BTreeMap::new(),
            speed_triggered_turn: None,
            current_speed_turn: None,
            declared_attackers_this_turn: BTreeSet::new(),
            once_this_turn: BTreeSet::new(),
            used_optional_trigger_effects: BTreeSet::new(),
            delayed_steps: vec![],
            delayed_event_triggers: vec![],
            next_spell_or_loyalty: Vec::new(),
            ability_copy_watches: vec![],
        }
    }
}
/// Also used by the explicitly pure operational boundary unit; never allocate a huge Vec.
pub(crate) fn next_occurrence_index(index: u64) -> Result<(u64, u64), TriggerError> {
    let next = index
        .checked_add(1)
        .ok_or(TriggerError::OccurrenceIndexExhausted)?;
    Ok((index, next))
}
impl TriggerState {
    pub(crate) fn retain_waiting_for(&mut self, players: &[SeatId]) {
        self.waiting
            .retain(|_, instance| players.contains(&instance.controller));
        self.speed_waiting
            .retain(|_, player| players.contains(player));
        self.monarch_waiting
            .retain(|_, (player, _)| players.contains(player));
        self.emblem_waiting
            .retain(|_, instance| players.contains(&instance.controller));
        self.delayed_copy_waiting
            .retain(|_, (player, _, _)| players.contains(player));
    }
    pub(crate) fn attack_allocation_mark(&self) -> [u64; 3] {
        [self.next_event, self.next_instance, self.next_wave]
    }
    pub(crate) fn retain_attack_mark(&mut self, mark: [u64; 3]) {
        self.next_event = self.next_event.max(mark[0]);
        self.next_instance = self.next_instance.max(mark[1]);
        self.next_wave = self.next_wave.max(mark[2]);
    }

    pub(crate) fn reset_once_each_turn(&mut self) {
        self.combat_draws.clear();
        self.once_this_turn.clear();
        self.used_optional_trigger_effects.clear();
        self.declared_attackers_this_turn.clear();
        self.ability_copy_watches.clear();
        self.next_spell_or_loyalty.clear();
    }
    pub(crate) fn optional_trigger_effect_used(
        &self,
        trigger: &TriggerBinding,
        controller: SeatId,
    ) -> bool {
        self.used_optional_trigger_effects.contains(&(
            trigger.source.object.id(),
            trigger.granting_effect,
            trigger.definition,
            controller,
        ))
    }
    pub(crate) fn use_optional_trigger_effect(
        &mut self,
        source: ObjectId,
        grant: Option<crate::characteristics::EffectInstanceKey>,
        definition: TriggeredAbilityId,
        controller: SeatId,
    ) {
        self.used_optional_trigger_effects
            .insert((source, grant, definition, controller));
    }

    pub(crate) fn watch_ability_copy(
        &mut self,
        source: SupportedAppearance,
        player: SeatId,
        watch: AbilityCopyWatch,
    ) {
        self.ability_copy_watches.push((source, player, watch));
    }
    pub(crate) fn expire_end_of_turn_watches(&mut self) {
        self.combat_draws.clear();
        self.ability_copy_watches.clear();
        self.next_spell_or_loyalty.clear();
        self.watched_graveyard_casts.clear();
    }

    pub(crate) fn watch_graveyard_cast(
        &mut self,
        player: SeatId,
        original: StackEntryId,
        recipient: ObjectId,
        card: ObjectId,
    ) {
        self.watched_graveyard_casts.push(GraveyardCastWatch {
            player,
            original,
            recipient,
            card,
        });
    }

    fn match_graveyard_cast(
        &mut self,
        occurrence: OccurrenceId,
        cast: &CastOccurrence,
        current: &RulesView<'_>,
        discoveries: &mut Vec<RulesRecord>,
    ) -> Result<(), TriggerError> {
        for watch in std::mem::take(&mut self.watched_graveyard_casts) {
            if Some(watch.card) != cast.cast_original_object || watch.player != cast.controller() {
                self.watched_graveyard_casts.push(watch);
                continue;
            }
            let donor = current
                .evaluated
                .get(cast.spell().object.id())
                .ok_or(TriggerError::InvalidWorld)?;
            let instance = TriggerInstanceId(self.next_instance);
            self.next_instance = self
                .next_instance
                .checked_add(1)
                .ok_or(TriggerError::InstanceIdentityExhausted)?;
            let action = DelayedCopyAction::GrantCastCardAbilities {
                recipient: watch.recipient,
                donor: Box::new(donor.clone()),
            };
            self.delayed_copy_waiting
                .insert(instance, (watch.player, watch.original, action));
            discoveries.push(RulesRecord::DelayedCopyTriggerDiscovered {
                instance,
                controller: watch.player,
                original: watch.original,
                cause: occurrence,
            });
        }
        Ok(())
    }
    pub(crate) fn watch_next_spell_or_loyalty(
        &mut self,
        player: SeatId,
        source: StackEntryId,
        colors: BTreeSet<crate::characteristics::Color>,
    ) {
        self.next_spell_or_loyalty.push((player, source, colors));
    }
    pub(crate) fn watches_next_spell(&self, player: SeatId) -> bool {
        self.next_spell_or_loyalty
            .iter()
            .any(|(controller, _, _)| *controller == player)
    }
    pub(crate) fn watches_ability_copies(&self) -> bool {
        !self.ability_copy_watches.is_empty() || !self.next_spell_or_loyalty.is_empty()
    }
    fn match_next_spell_or_loyalty(
        &mut self,
        id: OccurrenceId,
        controller: SeatId,
        journal: &mut Vec<RulesRecord>,
        action: impl Fn(BTreeSet<crate::characteristics::Color>) -> DelayedCopyAction,
    ) -> Result<(), TriggerError> {
        for (player, original, colors) in std::mem::take(&mut self.next_spell_or_loyalty) {
            if controller != player {
                self.next_spell_or_loyalty.push((player, original, colors));
                continue;
            }
            let instance = TriggerInstanceId(self.next_instance);
            self.next_instance = self
                .next_instance
                .checked_add(1)
                .ok_or(TriggerError::InstanceIdentityExhausted)?;
            self.delayed_copy_waiting
                .insert(instance, (player, original, action(colors)));
            journal.push(RulesRecord::DelayedCopyTriggerDiscovered {
                instance,
                controller,
                original,
                cause: id,
            });
        }
        Ok(())
    }
    pub(crate) fn discover_ability_copies(
        &mut self,
        occurrence: OccurrenceId,
        controller: SeatId,
        source: std::sync::Arc<crate::effects::CapturedAbilityCopy>,
        loyalty: bool,
        types: &BTreeSet<crate::program::PlaneswalkerSubtype>,
        journal: &mut RulesJournal,
    ) -> Result<(), TriggerError> {
        if loyalty {
            self.match_next_spell_or_loyalty(
                occurrence,
                controller,
                &mut journal.records,
                |colors| DelayedCopyAction::AbilityTwice(source.clone(), colors),
            )?;
        }
        for (appearance, player, watch) in std::mem::take(&mut self.ability_copy_watches) {
            let matches = player == controller
                && match watch {
                    AbilityCopyWatch::NextLoyalty => loyalty,
                    AbilityCopyWatch::PlaneswalkerType(kind) => types.contains(&kind),
                };
            if matches {
                let causes = TriggerCauseSet::new(
                    occurrence.event,
                    TriggerCauseKey::Occurrence(occurrence),
                    vec![TriggerCauseMember {
                        occurrence,
                        related_player: Some(player),
                        related: TriggerRelated::AbilityForCopy(source.clone()),
                    }],
                )?;
                let instance = WaitingInstance {
                    id: TriggerInstanceId(self.next_instance),
                    definition: TriggeredAbilityId(0),
                    granting_effect: None,
                    source: appearance.clone(),
                    controller: player,
                    causes,
                    body: ExecutableBody::new(
                        crate::program::BodyTarget::None,
                        vec![crate::program::Instruction::CopyCapturedAbility],
                    ),
                };
                self.next_instance = self
                    .next_instance
                    .checked_add(1)
                    .ok_or(TriggerError::InstanceIdentityExhausted)?;
                self.waiting.insert(instance.id, instance.clone());
                journal
                    .records
                    .push(RulesRecord::TriggerDiscovered(Box::new(instance)));
            }
            if !matches || watch != AbilityCopyWatch::NextLoyalty {
                self.ability_copy_watches.push((appearance, player, watch));
            }
        }
        Ok(())
    }
    /// Undo discoveries caused solely by a reversed cast announcement; later
    /// mana-ability events and their triggers remain intact.
    pub(crate) fn reverse_cast_announcement(&mut self, event: EventId) {
        self.waiting
            .retain(|_, instance| instance.causes.event() != event);
    }
    pub(crate) fn delay_token_leave(
        &mut self,
        token: ObjectId,
        source: SupportedAppearance,
        controller: SeatId,
    ) {
        self.token_leave_triggers
            .insert(token, (source, controller));
    }
    /// Schedule a delayed return trigger for an earthbent land if it dies or is exiled.
    pub fn delay_earthbend_return(
        &mut self,
        land: ObjectId,
        source: SupportedAppearance,
        controller: SeatId,
    ) {
        self.earthbent_returns.insert(land, (source, controller));
    }
    pub(crate) fn delay_return_with_counters(
        &mut self,
        source: SupportedAppearance,
        controller: SeatId,
        objects: Vec<ObjectId>,
    ) {
        self.delayed_steps.push(DelayedStepTrigger {
            source,
            controller,
            action: DelayedStepAction::ReturnExiledWithCounters(objects),
        });
    }
    pub(crate) fn delay_copy_return_with_counters(
        &mut self,
        original: StackEntryId,
        controller: SeatId,
        objects: Vec<ObjectId>,
    ) {
        self.delayed_copy_steps.push((
            controller,
            original,
            crate::turn::Step::End,
            DelayedCopyAction::ReturnExiledWithCounters(objects),
        ));
    }
    pub(crate) fn delay_copy_exile_return(
        &mut self,
        original: StackEntryId,
        controller: SeatId,
        objects: Vec<ObjectId>,
    ) {
        self.delayed_copy_steps.push((
            controller,
            original,
            crate::turn::Step::End,
            DelayedCopyAction::ReturnExiled(objects),
        ));
    }
    pub(crate) fn delay_copy_sacrifice(
        &mut self,
        original: StackEntryId,
        controller: SeatId,
        step: crate::turn::Step,
        objects: Vec<ObjectId>,
    ) {
        self.delayed_copy_steps.push((
            controller,
            original,
            step,
            DelayedCopyAction::Sacrifice(objects),
        ));
    }
    pub(crate) fn delay_copy_exile(
        &mut self,
        original: StackEntryId,
        controller: SeatId,
        step: crate::turn::Step,
        objects: Vec<ObjectId>,
    ) {
        self.delayed_copy_steps.push((
            controller,
            original,
            step,
            DelayedCopyAction::Exile(objects),
        ));
    }
    pub(crate) fn attacked_this_turn(&self, object: ObjectId) -> bool {
        self.declared_attackers_this_turn.contains(&object)
    }
    pub(crate) fn delayed_copy_waiting(
        &self,
    ) -> &BTreeMap<TriggerInstanceId, (SeatId, StackEntryId, DelayedCopyAction)> {
        &self.delayed_copy_waiting
    }
    pub(crate) fn take_delayed_copy(
        &mut self,
        id: TriggerInstanceId,
    ) -> Option<(SeatId, StackEntryId, DelayedCopyAction)> {
        self.delayed_copy_waiting.remove(&id)
    }
    pub(crate) fn delay_exile_return(
        &mut self,
        source: SupportedAppearance,
        controller: SeatId,
        objects: Vec<ObjectId>,
    ) {
        self.delayed_steps.push(DelayedStepTrigger {
            source,
            controller,
            action: DelayedStepAction::ReturnFromExile(objects),
        });
    }
    pub(crate) fn delay_graveyard_to_hand(
        &mut self,
        source: SupportedAppearance,
        controller: SeatId,
        object: ObjectId,
    ) {
        self.delayed_steps.push(DelayedStepTrigger {
            source,
            controller,
            action: DelayedStepAction::ReturnGraveyardToHand(object),
        });
    }
    pub(crate) fn delay_transformed_return(
        &mut self,
        source: SupportedAppearance,
        controller: SeatId,
        object: ObjectId,
    ) {
        self.delayed_steps.push(DelayedStepTrigger {
            source,
            controller,
            action: DelayedStepAction::ReturnFromGraveyardTransformed(object),
        });
    }
    pub(crate) fn delay_graveyard_return(
        &mut self,
        source: SupportedAppearance,
        controller: SeatId,
        object: ObjectId,
        while_source_remains: bool,
    ) {
        self.delayed_steps.push(DelayedStepTrigger {
            source,
            controller,
            action: DelayedStepAction::ReturnFromGraveyard {
                object,
                while_source_remains,
            },
        });
    }
    pub(crate) fn delay_phasing(
        &mut self,
        source: SupportedAppearance,
        controller: SeatId,
        objects: Vec<ObjectId>,
    ) {
        self.delayed_steps.push(DelayedStepTrigger {
            source,
            controller,
            action: DelayedStepAction::PhaseOut(objects),
        });
    }
    pub(crate) fn delay_permanent_return_to_hand(
        &mut self,
        source: SupportedAppearance,
        controller: SeatId,
        object: ObjectId,
    ) {
        self.delayed_steps.push(DelayedStepTrigger {
            source,
            controller,
            action: DelayedStepAction::ReturnPermanentToHand(object),
        });
    }
    pub(crate) fn delay_exile(
        &mut self,
        source: SupportedAppearance,
        controller: SeatId,
        step: crate::turn::Step,
        objects: Vec<ObjectId>,
    ) {
        self.delayed_steps.push(DelayedStepTrigger {
            source,
            controller,
            action: DelayedStepAction::Exile { step, objects },
        });
    }
    pub(crate) fn delay_sacrifice(
        &mut self,
        source: SupportedAppearance,
        controller: SeatId,
        step: crate::turn::Step,
        objects: Vec<ObjectId>,
    ) {
        self.delayed_steps.push(DelayedStepTrigger {
            source,
            controller,
            action: DelayedStepAction::Sacrifice { step, objects },
        });
    }
    pub(crate) fn delay_cast(
        &mut self,
        source: SupportedAppearance,
        object: ObjectId,
        kind: crate::program::EndStepCast,
        controller: SeatId,
    ) {
        self.delayed_steps.push(DelayedStepTrigger {
            source,
            controller,
            action: DelayedStepAction::Cast { object, kind },
        });
    }
    pub(crate) fn delay_unearth(
        &mut self,
        source: SupportedAppearance,
        object: ObjectId,
        controller: SeatId,
    ) {
        self.delayed_steps.push(DelayedStepTrigger {
            source,
            controller,
            action: DelayedStepAction::Unearth(object),
        });
    }
    pub(crate) fn delay_rebound(
        &mut self,
        source: SupportedAppearance,
        object: ObjectId,
        controller: SeatId,
    ) {
        self.delayed_steps.push(DelayedStepTrigger {
            source,
            controller,
            action: DelayedStepAction::Rebound(object),
        });
    }
    pub(crate) fn delay_step_body(
        &mut self,
        source: SupportedAppearance,
        controller: SeatId,
        step: crate::turn::Step,
        own_step_only: bool,
        body: Box<ExecutableBody>,
    ) {
        self.delayed_steps.push(DelayedStepTrigger {
            source,
            controller,
            action: DelayedStepAction::ExecuteBody {
                step,
                own_step_only,
                body,
            },
        });
    }
    pub(crate) fn delay_event_trigger(
        &mut self,
        source: SupportedAppearance,
        controller: SeatId,
        trigger: crate::program::TriggerSpec,
        body: ExecutableBody,
        expires: crate::characteristics::ResolvedDuration,
        once: bool,
    ) {
        self.delayed_event_triggers.push(DelayedEventTrigger {
            source,
            controller,
            trigger,
            body,
            expires,
            once,
        });
    }
    pub(crate) fn expire_delayed_event_triggers_at_cleanup(&mut self, active: SeatId, turn: u64) {
        self.delayed_event_triggers
            .retain(|delay| !delay.expires.expires_at_cleanup(active, turn));
    }
    pub(crate) fn expire_delayed_event_triggers_at_end_of_combat(&mut self) {
        self.delayed_event_triggers
            .retain(|delay| !delay.expires.expires_at_end_of_combat());
    }
    pub(crate) fn expire_delayed_event_triggers_at_turn_start(
        &mut self,
        active: SeatId,
        turn: u64,
    ) {
        self.delayed_event_triggers
            .retain(|delay| !delay.expires.expires_at_turn_start(active, turn));
    }
    pub(crate) fn counters(&self) -> (u64, u64, u64) {
        (self.next_event, self.next_instance, self.next_wave)
    }
    #[cfg(test)]
    pub(crate) fn set_test_next_event(&mut self, next: u64) {
        self.next_event = next;
    }
    #[cfg(test)]
    pub(crate) fn set_test_next_instance(&mut self, next: u64) {
        self.next_instance = next;
    }
    #[cfg(test)]
    pub(crate) fn set_test_next_wave(&mut self, next: u64) {
        self.next_wave = next;
    }
    pub(crate) fn enter_speed_turn(&mut self, number: u64, active: SeatId) {
        self.current_speed_turn = Some((number, active));
    }
    pub(crate) fn speed_waiting(&self) -> &BTreeMap<TriggerInstanceId, SeatId> {
        &self.speed_waiting
    }
    pub(crate) fn monarch_waiting(&self) -> &BTreeMap<TriggerInstanceId, (SeatId, Option<SeatId>)> {
        &self.monarch_waiting
    }
    pub(crate) fn take_monarch(
        &mut self,
        id: TriggerInstanceId,
    ) -> Option<(SeatId, Option<SeatId>)> {
        self.monarch_waiting.remove(&id)
    }
    pub(crate) fn weaken_opposing_attackers(
        &mut self,
        source: SupportedAppearance,
        controller: SeatId,
        opponents: BTreeSet<SeatId>,
    ) {
        self.weakening_attack_triggers
            .push((source, controller, opponents));
    }
    pub(crate) fn expire_attack_triggers(&mut self, active: SeatId, skipped: &BTreeSet<SeatId>) {
        self.weakening_attack_triggers
            .retain(|(_, controller, _)| *controller != active && !skipped.contains(controller));
    }
    pub(crate) fn delay_emblem_return(
        &mut self,
        emblem: crate::emblem::EmblemId,
        controller: SeatId,
        object: ObjectId,
    ) {
        self.delayed_emblem_returns
            .push((emblem, controller, object));
    }
    pub(crate) fn emblem_waiting(&self) -> &BTreeMap<TriggerInstanceId, WaitingEmblem> {
        &self.emblem_waiting
    }
    pub(crate) fn take_emblem(&mut self, id: TriggerInstanceId) -> Option<WaitingEmblem> {
        self.emblem_waiting.remove(&id)
    }
    pub(crate) fn take_speed(&mut self, id: TriggerInstanceId) -> Option<SeatId> {
        self.speed_waiting.remove(&id)
    }
    pub(crate) fn waiting_count(&self) -> usize {
        self.waiting.len()
            + self.speed_waiting.len()
            + self.monarch_waiting.len()
            + self.emblem_waiting.len()
            + self.delayed_copy_waiting.len()
    }
    pub(crate) fn waiting_controller(&self, id: TriggerInstanceId) -> Option<SeatId> {
        self.waiting
            .get(&id)
            .map(|x| x.controller)
            .or_else(|| self.speed_waiting.get(&id).copied())
            .or_else(|| {
                self.monarch_waiting
                    .get(&id)
                    .map(|(controller, _)| *controller)
            })
            .or_else(|| {
                self.emblem_waiting
                    .get(&id)
                    .map(|waiting| waiting.controller)
            })
            .or_else(|| self.delayed_copy_waiting.get(&id).map(|waiting| waiting.0))
    }
    /// CR 724.1a only removes triggered abilities which are already waiting.
    pub(crate) fn clear_waiting_at_end_turn(&mut self) {
        self.waiting.clear();
        self.speed_waiting.clear();
        self.monarch_waiting.clear();
        self.emblem_waiting.clear();
        self.delayed_copy_waiting.clear();
    }
    pub(crate) fn discard_undone_attack_taps(&mut self, ids: &BTreeSet<TriggerInstanceId>) {
        for id in ids {
            if let Some(instance) = self.waiting.remove(id) {
                self.once_this_turn.remove(&(
                    instance.source.object.id(),
                    instance.granting_effect,
                    instance.definition,
                ));
            }
        }
    }
    pub(crate) fn waiting(&self) -> &BTreeMap<TriggerInstanceId, WaitingInstance> {
        &self.waiting
    }
    pub(crate) fn start_wave(mut self) -> Result<(Self, PlacementWaveId), TriggerError> {
        let next = self
            .next_wave
            .checked_add(1)
            .ok_or(TriggerError::WaveIdentityExhausted)?;
        let wave = PlacementWaveId(self.next_wave);
        self.next_wave = next;
        Ok((self, wave))
    }
    pub(crate) fn take_waiting(
        mut self,
        ids: &[TriggerInstanceId],
    ) -> Result<(Self, Vec<WaitingInstance>), TriggerError> {
        let mut values = Vec::new();
        for id in ids {
            values.push(
                self.waiting
                    .remove(id)
                    .ok_or(TriggerError::InvalidTriggerOrder)?,
            );
        }
        Ok((self, values))
    }
    /// Consume only a tentative owning transaction. Failure returns no partial append/state.
    pub(crate) fn publish(
        mut self,
        cause: EventCause,
        values: Vec<RulesOccurrence>,
        before: &RulesView<'_>,
        after: &RulesView<'_>,
    ) -> Result<RulesTransition<Self, EventId>, TriggerError> {
        validate_event_shape(&values)?;
        let next = self
            .next_event
            .checked_add(1)
            .ok_or(TriggerError::EventIdentityExhausted)?;
        let id = EventId(self.next_event);
        self.next_event = next;
        let mut index = 0;
        let mut occurrences = Vec::new();
        for value in values {
            let (allocated, next) = next_occurrence_index(index)?;
            occurrences.push(EventOccurrence {
                id: OccurrenceId {
                    event: id,
                    index: allocated,
                },
                value,
            });
            index = next;
        }
        let event = RulesEvent {
            id,
            cause,
            occurrences,
        };
        let mut discoveries = Vec::new();
        for (source, _) in self.token_leave_triggers.values_mut() {
            if let Some(current) = before.appearance(source.object.id()) {
                *source = current;
            }
        }
        for (source, _) in self.earthbent_returns.values_mut() {
            if let Some(current) = before.appearance(source.object.id()) {
                *source = current;
            }
        }
        let mut sources = BTreeMap::new();
        let cycled: BTreeSet<_> = event
            .occurrences
            .iter()
            .filter_map(|row| match &row.value {
                RulesOccurrence::ZoneChange(change)
                    if matches!(change.reason, ZoneReason::CycleDiscard { .. }) =>
                {
                    Some(change.after.object.id())
                }
                _ => None,
            })
            .collect();
        let check_suspended = event.occurrences.iter().any(|row| match &row.value {
            RulesOccurrence::StepBegan { position, .. } => {
                position.step == crate::turn::Step::Upkeep
            }
            RulesOccurrence::TimeCounterRemoved { after, .. } => !after.is_positive(),
            _ => false,
        });
        for (view, lookback) in [(before, true), (after, false)] {
            for object in view.cards.iter().filter(|object| {
                matches!(
                    object.zone(),
                    ObjectZone::Battlefield
                        | ObjectZone::Stack
                        | ObjectZone::Graveyard(_)
                        | ObjectZone::Exile
                ) || (!lookback && cycled.contains(&object.id()))
            }) {
                let current = view
                    .evaluated
                    .get(object.id())
                    .ok_or(TriggerError::InvalidWorld)?;
                if current.object != object.id()
                    || current.controller != object.controller()
                    || current.abilities.source != object.id()
                    || current.abilities.card != object.card_id()
                {
                    return Err(TriggerError::InvalidWorld);
                }
                let suspend = !lookback
                    && object.zone() == ObjectZone::Exile
                    && check_suspended
                    && matches!(
                        current.printed_text.exile_action,
                        Some(crate::program::ExileAction::Suspend { .. })
                    );
                let has_triggers = !current.abilities.triggered.is_empty()
                    || !current.granted_triggers.is_empty()
                    || !current.granted_exalted.is_empty()
                    || !current.granted_combat_keywords.is_empty();
                if !suspend && !has_triggers {
                    continue;
                }
                let mut abilities = Vec::new();
                if suspend {
                    use crate::program::CounterKeyword;
                    let mut free_ids = (0..=u64::MAX).map(TriggeredAbilityId).filter(|id| {
                        !current
                            .printed_text
                            .triggered
                            .iter()
                            .any(|ability| ability.id == *id)
                    });
                    for kind in [
                        CounterKeyword::SuspendUpkeep,
                        CounterKeyword::SuspendLastCounter,
                    ] {
                        let id = free_ids
                            .next()
                            .ok_or(TriggerError::InstanceIdentityExhausted)?;
                        abilities.push((None, std::borrow::Cow::Owned(kind.ability(id))));
                    }
                }
                for (effect, granted) in &current.granted_triggers {
                    for ability in granted {
                        if ability.condition.functions_in_zone(object.zone())
                            && ability.condition.is_lookback() == lookback
                        {
                            abilities.push((Some(*effect), std::borrow::Cow::Borrowed(ability)));
                        }
                    }
                }
                for definition in &current.abilities.triggered {
                    let ability = current
                        .printed_text
                        .triggered
                        .iter()
                        .find(|ability| ability.id == *definition)
                        .ok_or(TriggerError::InvalidWorld)?;
                    if ability.condition.functions_in_zone(object.zone())
                        && ability.condition.is_lookback() == lookback
                    {
                        abilities.push((None, std::borrow::Cow::Borrowed(ability)));
                    }
                }
                if !lookback
                    && object.zone() == ObjectZone::Battlefield
                    && event
                        .occurrences
                        .iter()
                        .any(|row| matches!(row.value, RulesOccurrence::AttackersDeclared { .. }))
                {
                    for (&grant, &count) in &current.granted_exalted {
                        for ordinal in 0..count {
                            abilities.push((
                                Some(grant),
                                std::borrow::Cow::Owned(TriggeredAbility::exalted(
                                    TriggeredAbilityId(u64::from(ordinal)),
                                )),
                            ));
                        }
                    }
                }
                if !lookback
                    && object.zone() == ObjectZone::Battlefield
                    && event.occurrences.iter().any(|row| {
                        matches!(
                            row.value,
                            RulesOccurrence::CastCompleted(_)
                                | RulesOccurrence::DamageDealt { .. }
                                | RulesOccurrence::AttackersDeclared { .. }
                                | RulesOccurrence::BlockersDeclared { .. }
                        )
                    })
                {
                    for (&grant, keywords) in &current.granted_combat_keywords {
                        for (index, keyword) in keywords.iter().enumerate() {
                            // Exalted occupies [0, granted_exalted) in the same effect's definition namespace.
                            let exalted = current.granted_exalted.get(&grant).copied().unwrap_or(0);
                            let ordinal = u64::try_from(index)
                                .ok()
                                .and_then(|index| index.checked_add(u64::from(exalted)))
                                .ok_or(TriggerError::InstanceIdentityExhausted)?;
                            let ability = TriggeredAbility::combat_keyword(
                                TriggeredAbilityId(ordinal),
                                keyword.clone(),
                            );
                            abilities.push((Some(grant), std::borrow::Cow::Owned(ability)));
                        }
                    }
                }
                if abilities.is_empty() {
                    continue;
                }
                let source = view
                    .appearance(object.id())
                    .ok_or(TriggerError::InvalidWorld)?;
                let controller = current
                    .controller
                    .or_else(|| {
                        (suspend
                            || cycled.contains(&object.id())
                            || matches!(
                                object.zone(),
                                ObjectZone::Graveyard(_) | ObjectZone::Exile
                            ))
                        .then_some(object.owner())
                    })
                    .ok_or(TriggerError::InvalidWorld)?;
                for (grant, ability) in abilities {
                    sources.insert(
                        (object.id(), grant, ability.id),
                        (source.clone(), controller, ability),
                    );
                }
            }
        }
        // The spell owns each granted trigger. Eligibility is checked after all
        // casting costs have been paid, when a granting permanent may have left.
        for row in &event.occurrences {
            let RulesOccurrence::CastCompleted(cast) = &row.value else {
                continue;
            };
            let spell = cast.spell();
            for permanent in after
                .evaluated
                .objects
                .values()
                .filter(|p| p.zone == ObjectZone::Battlefield)
            {
                for rule in permanent
                    .printed_text
                    .continuous
                    .iter()
                    .chain(&permanent.printed_text.additional_battlefield_abilities)
                {
                    let (id, count) = match rule {
                        crate::characteristics::StaticContinuousAbility::CastPermission {
                            id,
                            permission:
                                crate::characteristics::CastPermissionRule::ColorlessHandSpellsHaveCascade {
                                    minimum_mana_value,
                                    count,
                                },
                        } => {
                            if permanent.controller != Some(cast.controller())
                                || !spell.colors.is_empty()
                                || spell.cast_origin != Some((cast.controller(), true))
                                || cast.mana_value() < &i64::from(*minimum_mana_value).into()
                            {
                                continue;
                            }
                            (*id, *count)
                        }
                        crate::characteristics::StaticContinuousAbility::CastPermission {
                            id,
                            permission:
                                crate::characteristics::CastPermissionRule::GrantSpellCascade {
                                    any_player,
                                    matching,
                                    count,
                                },
                        } => {
                            if (!*any_player && permanent.controller != Some(cast.controller()))
                                || !matching.matches_spell_appearance(spell)
                            {
                                continue;
                            }
                            (*id, *count)
                        }
                        _ => continue,
                    };
                    if !(permanent.abilities.continuous == Some(id)
                        || permanent
                            .abilities
                            .additional_battlefield_abilities
                            .contains(&id))
                    {
                        continue;
                    }
                    let grant = crate::characteristics::EffectInstanceKey::Static {
                        source: permanent.object,
                        ability: id,
                    };
                    for n in 0..count.get() {
                        let ability = TriggeredAbility::cascade(TriggeredAbilityId(u64::from(n)));
                        sources.insert(
                            (spell.object.id(), Some(grant), ability.id),
                            (
                                spell.clone(),
                                cast.controller(),
                                std::borrow::Cow::Owned(ability),
                            ),
                        );
                    }
                }
            }
        }
        self.match_combat_draws(&event, after, &mut discoveries)?;
        for ((object, granting_effect, ability_id), (source, controller, ability)) in sources {
            if ability
                .body
                .instructions
                .contains(&crate::program::Instruction::CopyEntrantOnceEachTurn)
                && self.used_optional_trigger_effects.contains(&(
                    object,
                    granting_effect,
                    ability_id,
                    controller,
                ))
            {
                continue;
            }
            if (matches!(
                ability.condition.event,
                TriggerEvent::Attacks {
                    mode: AttackTriggerMode::FirstThisTurn { .. },
                    ..
                }
            ) || ability.condition.intervening_if
                == Some(TriggerCondition::SourceDidNotAttackThisTurn))
                && self.attacked_this_turn(object)
            {
                continue;
            }
            let once_key = (object, granting_effect, ability_id);
            let once_each_turn = ability.grouping == TriggerGrouping::OneOrMoreOnceEachTurn;
            if once_each_turn && self.once_this_turn.contains(&once_key) {
                continue;
            }
            let mut members = Vec::new();
            members
                .try_reserve(event.occurrences.len())
                .map_err(|_| TriggerError::TriggerCauseCapacity)?;
            for occurrence in &event.occurrences {
                match_trigger_event(
                    &ability.condition.event,
                    ability.condition.intervening_if.as_ref(),
                    &ability,
                    &source,
                    controller,
                    occurrence,
                    &event.cause,
                    object,
                    ability_id,
                    granting_effect,
                    after,
                    &mut members,
                )?;
            }
            if once_each_turn && !members.is_empty() {
                self.once_this_turn.insert(once_key);
            }
            for causes in causes::group_matches(id, ability.grouping, members)? {
                let extra = additional_trigger_count(
                    ability.as_ref(),
                    &source,
                    controller,
                    &causes,
                    before,
                    after,
                );
                let mut body = ability.body.clone();
                use crate::program::BodyTarget;
                for target in std::iter::once(&mut body.target).chain(
                    body.triggered_modes.iter_mut().flat_map(|modes| {
                        modes.options.iter_mut().map(|option| &mut option.target)
                    }),
                ) {
                    *target = target.with_ability_source(source.object.id());
                    if let BodyTarget::FilteredPermanent(filter) = target
                        && filter.has_related_player()
                    {
                        let player = causes
                            .members()
                            .first()
                            .and_then(|c| c.related_player)
                            .ok_or(TriggerError::InvalidTriggerCauses)?;
                        if causes
                            .members()
                            .iter()
                            .any(|c| c.related_player != Some(player))
                        {
                            return Err(TriggerError::InvalidTriggerCauses);
                        }
                        *filter = filter.bind_related_player(player);
                    }
                }
                if let Some(crate::program::TriggerCondition::Eval(rc)) =
                    &ability.condition.intervening_if
                {
                    let prepend = |target_body: &mut ExecutableBody| {
                        for op in &mut target_body.instructions {
                            match op {
                                crate::program::Instruction::BranchIfFalse {
                                    destination, ..
                                }
                                | crate::program::Instruction::Jump { destination } => {
                                    *destination = destination.saturating_add(1);
                                }
                                _ => {}
                            }
                        }
                        let end = target_body.instructions.len().saturating_add(1);
                        target_body.instructions.insert(
                            0,
                            crate::program::Instruction::BranchIfFalse {
                                condition: rc.as_ref().clone(),
                                destination: end,
                            },
                        );
                    };
                    if let Some(modes) = &mut body.triggered_modes {
                        for option in &mut modes.options {
                            prepend(option);
                        }
                    } else {
                        prepend(&mut body);
                    }
                }
                for _ in 0..=extra {
                    let instance = WaitingInstance {
                        id: TriggerInstanceId(self.next_instance),
                        definition: ability.id,
                        granting_effect,
                        causes: causes.clone(),
                        source: source.clone(),
                        controller,
                        body: body.clone(),
                    };
                    self.next_instance = self
                        .next_instance
                        .checked_add(1)
                        .ok_or(TriggerError::InstanceIdentityExhausted)?;
                    self.waiting.insert(instance.id, instance.clone());
                    discoveries.push(RulesRecord::TriggerDiscovered(Box::new(instance)));
                }
            }
        }
        // Match active event-driven delayed triggers (CR 603.7).
        if !self.delayed_event_triggers.is_empty() {
            let existing = std::mem::take(&mut self.delayed_event_triggers);
            let mut retained = Vec::with_capacity(existing.len());
            for delay in existing {
                let synth_ability = crate::program::TriggeredAbility {
                    id: TriggeredAbilityId(0),
                    condition: delay.trigger.clone(),
                    grouping: TriggerGrouping::EachOccurrence,
                    body: delay.body.clone(),
                };
                let mut members = Vec::new();
                for occurrence in &event.occurrences {
                    match_trigger_event(
                        &synth_ability.condition.event,
                        synth_ability.condition.intervening_if.as_ref(),
                        &synth_ability,
                        &delay.source,
                        delay.controller,
                        occurrence,
                        &event.cause,
                        delay.source.object.id(),
                        synth_ability.id,
                        None,
                        after,
                        &mut members,
                    )?;
                }
                let fired = !members.is_empty();
                if fired {
                    for causes in
                        causes::group_matches(id, TriggerGrouping::EachOccurrence, members)?
                    {
                        let instance = WaitingInstance {
                            id: TriggerInstanceId(self.next_instance),
                            definition: TriggeredAbilityId(0),
                            granting_effect: None,
                            causes,
                            source: delay.source.clone(),
                            controller: delay.controller,
                            body: delay.body.clone(),
                        };
                        self.next_instance = self
                            .next_instance
                            .checked_add(1)
                            .ok_or(TriggerError::InstanceIdentityExhausted)?;
                        self.waiting.insert(instance.id, instance.clone());
                        discoveries.push(RulesRecord::TriggerDiscovered(Box::new(instance)));
                    }
                }
                if !(delay.once && fired) {
                    retained.push(delay);
                }
            }
            self.delayed_event_triggers = retained;
        }
        for occurrence in &event.occurrences {
            if let RulesOccurrence::CastCompleted(cast) = &occurrence.value
                && let Some(stack) = after
                    .window
                    .entries()
                    .iter()
                    .find(|entry| entry.id() == cast.entry())
                && !stack.granted_replicate.is_empty()
                && let Some(spell) = after.evaluated.get(cast.spell().object.id())
            {
                for grant in after.evaluated.replicate_grants(cast.controller(), spell) {
                    let Some(&index) = stack.granted_replicate.get(&grant) else {
                        continue;
                    };
                    let ability = TriggeredAbility::spell_copies(
                        TriggeredAbilityId(0),
                        crate::program::CastCopyKeyword::Replicate(index),
                    );
                    let causes = TriggerCauseSet::new(
                        id,
                        TriggerCauseKey::Occurrence(occurrence.id),
                        vec![TriggerCauseMember {
                            occurrence: occurrence.id,
                            related_player: Some(cast.controller()),
                            related: TriggerRelated::SpellCast(cast.clone()),
                        }],
                    )?;
                    let instance = WaitingInstance {
                        id: TriggerInstanceId(self.next_instance),
                        definition: ability.id,
                        granting_effect: Some(grant),
                        causes,
                        source: cast.spell().clone(),
                        controller: cast.controller(),
                        body: ability.body,
                    };
                    self.next_instance = self
                        .next_instance
                        .checked_add(1)
                        .ok_or(TriggerError::InstanceIdentityExhausted)?;
                    self.waiting.insert(instance.id, instance.clone());
                    discoveries.push(RulesRecord::TriggerDiscovered(Box::new(instance)));
                }
            }
            if let RulesOccurrence::CastCompleted(cast) = &occurrence.value
                && cast.cast_from_graveyard
                && !self.watched_graveyard_casts.is_empty()
            {
                self.match_graveyard_cast(occurrence.id, cast, after, &mut discoveries)?;
            }
            if let RulesOccurrence::CastCompleted(cast) = &occurrence.value
                && SpellFilter::instant_or_sorcery().matches_spell_appearance(cast.spell())
                && self.watches_next_spell(cast.controller())
            {
                if cast.copy_source.is_none() {
                    return Err(TriggerError::InvalidWorld);
                }
                self.match_next_spell_or_loyalty(
                    occurrence.id,
                    cast.controller(),
                    &mut discoveries,
                    |colors| DelayedCopyAction::SpellTwice(cast.clone(), colors),
                )?;
            }
            if let RulesOccurrence::ZoneChange(change) = &occurrence.value
                && change.before.object.zone() == ObjectZone::Battlefield
                && change.after.object.zone() != ObjectZone::Battlefield
                && let Some((source, controller)) =
                    self.token_leave_triggers.remove(&change.before.object.id())
            {
                let causes = TriggerCauseSet::new(
                    id,
                    TriggerCauseKey::Occurrence(occurrence.id),
                    vec![TriggerCauseMember {
                        occurrence: occurrence.id,
                        related_player: Some(controller),
                        related: TriggerRelated::ZoneChange {
                            before: change.before.clone(),
                            after: change.after.clone(),
                        },
                    }],
                )?;
                let instance = WaitingInstance {
                    id: TriggerInstanceId(self.next_instance),
                    definition: TriggeredAbilityId(0),
                    granting_effect: None,
                    causes,
                    source,
                    controller,
                    body: ExecutableBody::new(
                        crate::program::BodyTarget::None,
                        vec![crate::program::Instruction::ReturnTokenLinkedExile],
                    ),
                };
                self.next_instance = self
                    .next_instance
                    .checked_add(1)
                    .ok_or(TriggerError::InstanceIdentityExhausted)?;
                self.waiting.insert(instance.id, instance.clone());
                discoveries.push(RulesRecord::TriggerDiscovered(Box::new(instance)));
            }
            if let RulesOccurrence::ZoneChange(change) = &occurrence.value
                && change.before.object.zone() == ObjectZone::Battlefield
                && change.after.object.zone() != ObjectZone::Battlefield
                && let Some((source, controller)) =
                    self.earthbent_returns.remove(&change.before.object.id())
                && !change.before.object.is_token()
                && matches!(
                    change.after.object.zone(),
                    ObjectZone::Graveyard(_) | ObjectZone::Exile
                )
            {
                let causes = TriggerCauseSet::new(
                    id,
                    TriggerCauseKey::Occurrence(occurrence.id),
                    vec![TriggerCauseMember {
                        occurrence: occurrence.id,
                        related_player: Some(controller),
                        related: TriggerRelated::ZoneChange {
                            before: change.before.clone(),
                            after: change.after.clone(),
                        },
                    }],
                )?;
                let instance = WaitingInstance {
                    id: TriggerInstanceId(self.next_instance),
                    definition: TriggeredAbilityId(0),
                    granting_effect: None,
                    causes,
                    source,
                    controller,
                    body: ExecutableBody::new(
                        crate::program::BodyTarget::None,
                        vec![crate::program::Instruction::ReturnEarthbentCard],
                    ),
                };
                self.next_instance = self
                    .next_instance
                    .checked_add(1)
                    .ok_or(TriggerError::InstanceIdentityExhausted)?;
                self.waiting.insert(instance.id, instance.clone());
                discoveries.push(RulesRecord::TriggerDiscovered(Box::new(instance)));
            }
        }
        for occurrence in &event.occurrences {
            if let RulesOccurrence::AttackersDeclared { selected, .. } = &occurrence.value {
                for (source, controller, opponents) in &self.weakening_attack_triggers {
                    for attack in selected {
                        let Some(attacker) = after.evaluated.get(attack.attacker) else {
                            return Err(TriggerError::InvalidWorld);
                        };
                        if !attacker.is_creature()
                            || !attacker
                                .controller
                                .is_some_and(|owner| opponents.contains(&owner))
                        {
                            continue;
                        }
                        let causes = TriggerCauseSet::new(
                            id,
                            TriggerCauseKey::Occurrence(occurrence.id),
                            vec![TriggerCauseMember {
                                occurrence: occurrence.id,
                                related_player: attacker.controller,
                                related: TriggerRelated::Combatant(attack.attacker),
                            }],
                        )?;
                        let instance = WaitingInstance {
                            id: TriggerInstanceId(self.next_instance),
                            definition: TriggeredAbilityId(0),
                            granting_effect: None,
                            causes,
                            source: source.clone(),
                            controller: *controller,
                            body: ExecutableBody::new(
                                crate::program::BodyTarget::None,
                                vec![crate::program::Instruction::WeakenDelayedAttacker(
                                    attack.attacker,
                                )],
                            ),
                        };
                        self.next_instance = self
                            .next_instance
                            .checked_add(1)
                            .ok_or(TriggerError::InstanceIdentityExhausted)?;
                        self.waiting.insert(instance.id, instance.clone());
                        discoveries.push(RulesRecord::TriggerDiscovered(Box::new(instance)));
                    }
                }
            }
            if let RulesOccurrence::CommanderTypeManaSpent {
                source,
                player,
                units,
            } = &occurrence.value
            {
                let causes = TriggerCauseSet::new(
                    id,
                    TriggerCauseKey::Occurrence(occurrence.id),
                    vec![TriggerCauseMember {
                        occurrence: occurrence.id,
                        related_player: Some(*player),
                        related: TriggerRelated::ManaSource(source.object.id()),
                    }],
                )?;
                for _ in 0..units
                    .to_usize()
                    .ok_or(TriggerError::InstanceIdentityExhausted)?
                {
                    let instance = WaitingInstance {
                        id: TriggerInstanceId(self.next_instance),
                        definition: TriggeredAbilityId(0),
                        granting_effect: None,
                        causes: causes.clone(),
                        source: source.as_ref().clone(),
                        controller: *player,
                        body: ExecutableBody::new(
                            crate::program::BodyTarget::None,
                            vec![crate::program::Instruction::Scry {
                                player: crate::program::ResolvingPlayer::Controller,
                                amount: crate::program::ValueExpr::Literal(1.into()),
                            }],
                        ),
                    };
                    self.next_instance = self
                        .next_instance
                        .checked_add(1)
                        .ok_or(TriggerError::InstanceIdentityExhausted)?;
                    self.waiting.insert(instance.id, instance.clone());
                    discoveries.push(RulesRecord::TriggerDiscovered(Box::new(instance)));
                }
            }
            if let RulesOccurrence::GraveyardReturnPaid {
                source,
                controller,
                mana_value,
            } = &occurrence.value
            {
                let causes = TriggerCauseSet::new(
                    id,
                    TriggerCauseKey::Occurrence(occurrence.id),
                    vec![TriggerCauseMember {
                        occurrence: occurrence.id,
                        related_player: Some(*controller),
                        related: TriggerRelated::GraveyardReturnPaid {
                            mana_value: mana_value.clone(),
                        },
                    }],
                )?;
                let instance = WaitingInstance {
                    id: TriggerInstanceId(self.next_instance),
                    definition: TriggeredAbilityId(0),
                    granting_effect: None,
                    causes,
                    source: source.as_ref().clone(),
                    controller: *controller,
                    body: ExecutableBody::new(
                        crate::program::BodyTarget::FilteredOwnGraveyardCard(
                            crate::program::ObjectFilter::creature(),
                        ),
                        vec![
                            crate::program::Instruction::ReturnGraveyardCreatureWithCorpse(
                                mana_value.clone(),
                            ),
                        ],
                    ),
                };
                self.next_instance = self
                    .next_instance
                    .checked_add(1)
                    .ok_or(TriggerError::InstanceIdentityExhausted)?;
                self.waiting.insert(instance.id, instance.clone());
                discoveries.push(RulesRecord::TriggerDiscovered(Box::new(instance)));
            }
            if let RulesOccurrence::ReflexiveTrigger {
                source,
                controller,
                body,
                chosen_x,
            } = &occurrence.value
            {
                let causes = TriggerCauseSet::new(
                    id,
                    TriggerCauseKey::Occurrence(occurrence.id),
                    vec![TriggerCauseMember {
                        occurrence: occurrence.id,
                        related_player: Some(*controller),
                        related: TriggerRelated::ReflexiveTrigger {
                            chosen_x: chosen_x.clone(),
                        },
                    }],
                )?;
                let mut instance_body = body.as_ref().clone();
                instance_body.target = instance_body.target.with_ability_source(source.object.id());
                let instance = WaitingInstance {
                    id: TriggerInstanceId(self.next_instance),
                    definition: TriggeredAbilityId(0),
                    granting_effect: None,
                    causes,
                    source: source.as_ref().clone(),
                    controller: *controller,
                    body: instance_body,
                };
                self.next_instance = self
                    .next_instance
                    .checked_add(1)
                    .ok_or(TriggerError::InstanceIdentityExhausted)?;
                self.waiting.insert(instance.id, instance.clone());
                discoveries.push(RulesRecord::TriggerDiscovered(Box::new(instance)));
            }
            if let RulesOccurrence::ReflexiveDiscardDamage {
                source,
                controller,
                amount,
            } = &occurrence.value
            {
                let causes = TriggerCauseSet::new(
                    id,
                    TriggerCauseKey::Occurrence(occurrence.id),
                    vec![TriggerCauseMember {
                        occurrence: occurrence.id,
                        related_player: Some(*controller),
                        related: TriggerRelated::ReflexiveDiscardDamage {
                            amount: amount.clone(),
                        },
                    }],
                )?;
                let instance = WaitingInstance {
                    id: TriggerInstanceId(self.next_instance),
                    definition: TriggeredAbilityId(0),
                    granting_effect: None,
                    causes,
                    source: source.as_ref().clone(),
                    controller: *controller,
                    body: ExecutableBody::new(
                        crate::program::BodyTarget::FilteredPermanent(
                            crate::program::ObjectFilter::creature_or_planeswalker(),
                        ),
                        vec![crate::program::Instruction::DamageAmount(
                            crate::program::ValueExpr::Literal(amount.clone()),
                        )],
                    ),
                };
                self.next_instance = self
                    .next_instance
                    .checked_add(1)
                    .ok_or(TriggerError::InstanceIdentityExhausted)?;
                self.waiting.insert(instance.id, instance.clone());
                discoveries.push(RulesRecord::TriggerDiscovered(Box::new(instance)));
            }
            if let RulesOccurrence::MiracleRevealed { object } = &occurrence.value {
                let controller = object.object.owner();
                if object.object.zone() != ObjectZone::Hand(controller)
                    || object.printed_text.miracle.is_none()
                {
                    return Err(TriggerError::InvalidWorld);
                }
                let causes = TriggerCauseSet::new(
                    id,
                    TriggerCauseKey::Occurrence(occurrence.id),
                    vec![TriggerCauseMember {
                        occurrence: occurrence.id,
                        related_player: Some(controller),
                        related: TriggerRelated::MiracleRevealed(object.object.id()),
                    }],
                )?;
                let instance = WaitingInstance {
                    id: TriggerInstanceId(self.next_instance),
                    definition: TriggeredAbilityId(0),
                    granting_effect: None,
                    causes,
                    source: object.as_ref().clone(),
                    controller,
                    body: ExecutableBody::new(
                        crate::program::BodyTarget::None,
                        vec![crate::program::Instruction::MiracleCast(object.object.id())],
                    ),
                };
                self.next_instance = self
                    .next_instance
                    .checked_add(1)
                    .ok_or(TriggerError::InstanceIdentityExhausted)?;
                self.waiting.insert(instance.id, instance.clone());
                discoveries.push(RulesRecord::TriggerDiscovered(Box::new(instance)));
            }
            if let RulesOccurrence::ZoneChange(change) = &occurrence.value
                && change.madness
            {
                let source = &change.after;
                let controller = source.object.owner();
                if source.object.zone() != ObjectZone::Exile
                    || change.before.object.zone() != ObjectZone::Hand(controller)
                    || change.before.printed_text.madness.is_none()
                {
                    return Err(TriggerError::InvalidWorld);
                }
                let causes = TriggerCauseSet::new(
                    id,
                    TriggerCauseKey::Occurrence(occurrence.id),
                    vec![TriggerCauseMember {
                        occurrence: occurrence.id,
                        related_player: Some(controller),
                        related: TriggerRelated::Discard {
                            player: controller,
                            before: change.before.clone(),
                            after: source.clone(),
                        },
                    }],
                )?;
                let instance = WaitingInstance {
                    id: TriggerInstanceId(self.next_instance),
                    definition: TriggeredAbilityId(0),
                    granting_effect: None,
                    causes,
                    source: source.clone(),
                    controller,
                    body: ExecutableBody::new(
                        crate::program::BodyTarget::None,
                        vec![crate::program::Instruction::MadnessCast],
                    ),
                };
                self.next_instance = self
                    .next_instance
                    .checked_add(1)
                    .ok_or(TriggerError::InstanceIdentityExhausted)?;
                self.waiting.insert(instance.id, instance.clone());
                discoveries.push(RulesRecord::TriggerDiscovered(Box::new(instance)));
            }
            if let RulesOccurrence::StepBegan { position, active } = &occurrence.value {
                use crate::{program::Instruction, turn::Step};
                if position.step == Step::End {
                    for (emblem, controller, object) in
                        std::mem::take(&mut self.delayed_emblem_returns)
                    {
                        let instance = TriggerInstanceId(self.next_instance);
                        self.next_instance = self
                            .next_instance
                            .checked_add(1)
                            .ok_or(TriggerError::InstanceIdentityExhausted)?;
                        self.emblem_waiting.insert(
                            instance,
                            WaitingEmblem {
                                emblem,
                                controller,
                                body: ExecutableBody::new(
                                    crate::program::BodyTarget::None,
                                    vec![Instruction::ReturnDelayedGraveyardCard(object)],
                                ),
                                trigger: None,
                            },
                        );
                        discoveries.push(RulesRecord::EmblemTriggerDiscovered {
                            instance,
                            controller,
                            emblem,
                            cause: occurrence.id,
                        });
                    }
                }
                let ready = self
                    .delayed_steps
                    .extract_if(.., |delay| match &delay.action {
                        DelayedStepAction::Exile { step, .. }
                        | DelayedStepAction::Sacrifice { step, .. } => position.step == *step,
                        DelayedStepAction::ReturnExiledWithCounters(_)
                        | DelayedStepAction::PhaseOut(_)
                        | DelayedStepAction::ReturnGraveyardToHand(_)
                        | DelayedStepAction::ReturnPermanentToHand(_)
                        | DelayedStepAction::ReturnFromExile(_)
                        | DelayedStepAction::ReturnFromGraveyard { .. }
                        | DelayedStepAction::ReturnFromGraveyardTransformed(_)
                        | DelayedStepAction::Cast { .. }
                        | DelayedStepAction::Unearth(_) => position.step == Step::End,
                        DelayedStepAction::Rebound(_) => {
                            position.step == Step::Upkeep && *active == delay.controller
                        }
                        DelayedStepAction::ExecuteBody {
                            step,
                            own_step_only,
                            ..
                        } => {
                            position.step == *step
                                && (!*own_step_only || *active == delay.controller)
                        }
                    })
                    .collect::<Vec<_>>();
                let copies = self
                    .delayed_copy_steps
                    .extract_if(.., |(_, _, step, _)| position.step == *step)
                    .collect::<Vec<_>>();
                for (controller, original, _, action) in copies {
                    let instance = TriggerInstanceId(self.next_instance);
                    self.next_instance = self
                        .next_instance
                        .checked_add(1)
                        .ok_or(TriggerError::InstanceIdentityExhausted)?;
                    self.delayed_copy_waiting
                        .insert(instance, (controller, original, action));
                    discoveries.push(RulesRecord::DelayedCopyTriggerDiscovered {
                        instance,
                        controller,
                        original,
                        cause: occurrence.id,
                    });
                }
                for DelayedStepTrigger {
                    source,
                    controller,
                    action,
                } in ready
                {
                    let delayed_body = if let DelayedStepAction::ExecuteBody { body, .. } = action {
                        let mut b = *body;
                        b.target = b.target.with_ability_source(source.object.id());
                        b
                    } else {
                        let instructions = match action {
                            DelayedStepAction::Exile { objects, .. } => {
                                vec![Instruction::ExileDelayedObjects(objects)]
                            }
                            DelayedStepAction::Sacrifice { objects, .. } => {
                                vec![Instruction::SacrificeDelayedObjects(objects)]
                            }
                            DelayedStepAction::Cast { object, kind } => {
                                vec![Instruction::EndStepCast {
                                    kind,
                                    permanent: object,
                                    turn: position.number,
                                }]
                            }
                            DelayedStepAction::ReturnExiledWithCounters(objects) => {
                                vec![Instruction::ReturnDelayedExileWithCounters(objects)]
                            }
                            DelayedStepAction::PhaseOut(objects) => {
                                vec![Instruction::PhaseOutObjects(objects)]
                            }
                            DelayedStepAction::ReturnGraveyardToHand(object) => {
                                vec![Instruction::ReturnDelayedGraveyardToHand(object)]
                            }
                            DelayedStepAction::ReturnPermanentToHand(object) => {
                                vec![Instruction::ReturnDelayedPermanentToHand(object)]
                            }
                            DelayedStepAction::ReturnFromExile(object) => {
                                vec![Instruction::ReturnDelayedExile(object)]
                            }
                            DelayedStepAction::Unearth(object) => {
                                vec![Instruction::ExileUnearthed(object)]
                            }
                            DelayedStepAction::Rebound(object) => {
                                vec![Instruction::ReboundCast(object)]
                            }
                            DelayedStepAction::ReturnFromGraveyardTransformed(object) => {
                                vec![Instruction::ReturnDelayedGraveyardCardTransformed(object)]
                            }
                            DelayedStepAction::ReturnFromGraveyard {
                                object,
                                while_source_remains: true,
                            } => {
                                if !after
                                    .cards
                                    .get(source.object.id())
                                    .is_some_and(|card| card.zone() == ObjectZone::Battlefield)
                                {
                                    continue;
                                }
                                vec![
                                    Instruction::RequireCondition(
                                        crate::program::InterveningCondition::ReturnSourceOnBattlefield,
                                    ),
                                    Instruction::OptionalEffect {
                                        chooser: crate::program::ResolvingPlayer::Controller,
                                    },
                                    Instruction::ReturnDelayedGraveyardCard(object),
                                ]
                            }
                            DelayedStepAction::ReturnFromGraveyard {
                                object,
                                while_source_remains: false,
                            } => vec![Instruction::ReturnDelayedGraveyardCard(object)],
                            DelayedStepAction::ExecuteBody { .. } => unreachable!(),
                        };
                        ExecutableBody::new(crate::program::BodyTarget::None, instructions)
                    };
                    let causes = TriggerCauseSet::new(
                        id,
                        TriggerCauseKey::Occurrence(occurrence.id),
                        vec![TriggerCauseMember {
                            occurrence: occurrence.id,
                            related_player: Some(*active),
                            related: TriggerRelated::Step {
                                position: *position,
                                active: *active,
                            },
                        }],
                    )?;
                    let instance = WaitingInstance {
                        id: TriggerInstanceId(self.next_instance),
                        definition: TriggeredAbilityId(0),
                        granting_effect: None,
                        causes,
                        source,
                        controller,
                        body: delayed_body,
                    };
                    self.next_instance = self
                        .next_instance
                        .checked_add(1)
                        .ok_or(TriggerError::InstanceIdentityExhausted)?;
                    self.waiting.insert(instance.id, instance.clone());
                    discoveries.push(RulesRecord::TriggerDiscovered(Box::new(instance)));
                }
            }
        }
        if let Some(turn_id @ (_, active)) = self.current_speed_turn {
            let speed = before
                .characteristics
                .speed
                .get(&active)
                .copied()
                .unwrap_or(0);
            if (1..4).contains(&speed) && self.speed_triggered_turn != Some(turn_id) {
                let causes=event.occurrences.iter().filter_map(|row|matches!(&row.value,RulesOccurrence::LifeChanged(change) if change.kind==crate::life::LifeChangeKind::Loss && change.player!=active).then_some(row.id)).collect::<Vec<_>>();
                if !causes.is_empty() {
                    let instance = TriggerInstanceId(self.next_instance);
                    self.next_instance = self
                        .next_instance
                        .checked_add(1)
                        .ok_or(TriggerError::InstanceIdentityExhausted)?;
                    self.speed_triggered_turn = Some(turn_id);
                    self.speed_waiting.insert(instance, active);
                    discoveries.push(RulesRecord::SpeedTriggerDiscovered {
                        instance,
                        controller: active,
                        causes,
                    });
                }
            }
        }
        if let Some(monarch) = before.characteristics.monarch {
            for row in &event.occurrences {
                let action = match &row.value {
                    RulesOccurrence::StepBegan { position, active }
                        if *active == monarch && position.step == crate::turn::Step::End =>
                    {
                        Some(None)
                    }
                    RulesOccurrence::DamageDealt {
                        damage,
                        kind: DamageKind::Combat,
                        ..
                    } if damage.target == crate::damage::DamageRecipient::Player(monarch)
                        && matches!(damage.source.id(), crate::damage::DamageSourceId::Object(id) if before.evaluated.get(id).is_some_and(|object| object.is_creature())) =>
                    {
                        Some(Some(damage.source.controller()))
                    }
                    _ => None,
                };
                if let Some(beneficiary) = action {
                    let instance = TriggerInstanceId(self.next_instance);
                    self.next_instance = self
                        .next_instance
                        .checked_add(1)
                        .ok_or(TriggerError::InstanceIdentityExhausted)?;
                    self.monarch_waiting
                        .insert(instance, (monarch, beneficiary));
                    discoveries.push(RulesRecord::MonarchTriggerDiscovered {
                        instance,
                        controller: monarch,
                        beneficiary,
                        causes: vec![row.id],
                    });
                }
            }
        }
        for row in &event.occurrences {
            let RulesOccurrence::StepBegan { position, active } = &row.value else {
                continue;
            };
            if position.step != crate::turn::Step::Upkeep {
                continue;
            }
            for emblem in before.characteristics.emblems.values() {
                let crate::emblem::EmblemDefinition::UpkeepDamage { amount } = &emblem.definition
                else {
                    continue;
                };
                if emblem.controller != *active {
                    continue;
                }
                let instance = TriggerInstanceId(self.next_instance);
                self.next_instance = self
                    .next_instance
                    .checked_add(1)
                    .ok_or(TriggerError::InstanceIdentityExhausted)?;
                self.emblem_waiting.insert(
                    instance,
                    WaitingEmblem {
                        controller: emblem.controller,
                        emblem: emblem.id,
                        body: ExecutableBody::new(
                            crate::program::BodyTarget::None,
                            vec![crate::program::Instruction::DamageGroup {
                                group: crate::program::DamageGroup::Controller,
                                amount: amount.clone(),
                            }],
                        ),
                        trigger: None,
                    },
                );
                discoveries.push(RulesRecord::EmblemTriggerDiscovered {
                    instance,
                    controller: emblem.controller,
                    emblem: emblem.id,
                    cause: row.id,
                });
            }
        }
        for row in &event.occurrences {
            let RulesOccurrence::ZoneChange(change) = &row.value else {
                continue;
            };
            let ObjectZone::Graveyard(owner) = change.after.object.zone() else {
                continue;
            };
            for emblem in after
                .characteristics
                .emblems
                .values()
                .filter(|emblem| emblem.controller == owner)
            {
                use crate::{
                    emblem::EmblemDefinition,
                    program::{BodyTarget, Instruction, ResolvingPlayer},
                };
                let object = change.after.object.id();
                let instructions = match emblem.definition {
                    EmblemDefinition::ReturnFallenArtifacts
                        if change.before.object.zone() == ObjectZone::Battlefield
                            && change.before.types.contains(&CardType::Artifact) =>
                    {
                        vec![Instruction::ScheduleArtifactReturn(object)]
                    }
                    EmblemDefinition::UnlimitedHandWithGraveyardReturn
                        if change.after.object.card_id().is_some() =>
                    {
                        vec![
                            Instruction::OptionalSequence {
                                chooser: ResolvingPlayer::Controller,
                                instructions: 1,
                            },
                            Instruction::ReturnDelayedGraveyardToHand(object),
                        ]
                    }
                    _ => continue,
                };
                let instance = TriggerInstanceId(self.next_instance);
                self.next_instance = self
                    .next_instance
                    .checked_add(1)
                    .ok_or(TriggerError::InstanceIdentityExhausted)?;
                self.emblem_waiting.insert(
                    instance,
                    WaitingEmblem {
                        emblem: emblem.id,
                        controller: owner,
                        body: ExecutableBody::new(BodyTarget::None, instructions),
                        trigger: None,
                    },
                );
                discoveries.push(RulesRecord::EmblemTriggerDiscovered {
                    instance,
                    controller: owner,
                    emblem: emblem.id,
                    cause: row.id,
                });
            }
        }
        for row in &event.occurrences {
            let RulesOccurrence::CastCompleted(cast) = &row.value else {
                continue;
            };
            for emblem in before.characteristics.emblems.values() {
                let crate::emblem::EmblemDefinition::SpellcastDamage {
                    amount,
                    noncreature_only,
                } = &emblem.definition
                else {
                    continue;
                };
                if cast.controller() != emblem.controller
                    || (*noncreature_only && cast.spell().is_creature())
                {
                    continue;
                }
                let instance = TriggerInstanceId(self.next_instance);
                self.next_instance = self
                    .next_instance
                    .checked_add(1)
                    .ok_or(TriggerError::InstanceIdentityExhausted)?;
                self.emblem_waiting.insert(
                    instance,
                    WaitingEmblem {
                        controller: emblem.controller,
                        emblem: emblem.id,
                        body: ExecutableBody::new(
                            crate::program::BodyTarget::SingleAnyTarget,
                            vec![crate::program::Instruction::Damage {
                                amount: amount.clone(),
                            }],
                        ),
                        trigger: None,
                    },
                );
                discoveries.push(RulesRecord::EmblemTriggerDiscovered {
                    instance,
                    controller: emblem.controller,
                    emblem: emblem.id,
                    cause: row.id,
                });
            }
        }
        for emblem in before.characteristics.emblems.values() {
            let crate::emblem::EmblemDefinition::Authored { triggered, .. } = &emblem.definition
            else {
                continue;
            };
            if triggered.is_empty() {
                continue;
            }
            let synth = SupportedAppearance::synthetic_emblem(emblem.controller);
            let synth_id = synth.object.id();
            for ability in triggered {
                let mut members = Vec::new();
                for row in &event.occurrences {
                    matching::match_trigger_event(
                        &ability.condition.event,
                        ability.condition.intervening_if.as_ref(),
                        ability,
                        &synth,
                        emblem.controller,
                        row,
                        &event.cause,
                        synth_id,
                        ability.id,
                        None,
                        after,
                        &mut members,
                    )?;
                }
                if members.is_empty() {
                    continue;
                }
                let groups: Vec<(TriggerCauseKey, Vec<TriggerCauseMember>)> = match ability.grouping
                {
                    crate::program::TriggerGrouping::EachOccurrence => members
                        .into_iter()
                        .map(|m| (TriggerCauseKey::Occurrence(m.occurrence), vec![m]))
                        .collect(),
                    crate::program::TriggerGrouping::OneOrMoreTogether
                    | crate::program::TriggerGrouping::OneOrMoreOnceEachTurn
                    | crate::program::TriggerGrouping::OneOrMoreByPlayer => {
                        vec![(TriggerCauseKey::Together, members)]
                    }
                };
                for (cause_key, group) in groups {
                    let Some(first_cause) = group.first().map(|m| m.occurrence) else {
                        continue;
                    };
                    let causes = TriggerCauseSet::new(event.id, cause_key, group)?;
                    let instance = TriggerInstanceId(self.next_instance);
                    self.next_instance = self
                        .next_instance
                        .checked_add(1)
                        .ok_or(TriggerError::InstanceIdentityExhausted)?;
                    let trigger_binding = TriggerBinding {
                        instance,
                        definition: ability.id,
                        granting_effect: None,
                        causes,
                        source: synth.clone(),
                    };
                    self.emblem_waiting.insert(
                        instance,
                        WaitingEmblem {
                            controller: emblem.controller,
                            emblem: emblem.id,
                            body: ability.body.clone(),
                            trigger: Some(trigger_binding),
                        },
                    );
                    discoveries.push(RulesRecord::EmblemTriggerDiscovered {
                        instance,
                        controller: emblem.controller,
                        emblem: emblem.id,
                        cause: first_cause,
                    });
                }
            }
        }
        for occurrence in &event.occurrences {
            if let RulesOccurrence::AttackersDeclared { selected, .. } = &occurrence.value {
                self.declared_attackers_this_turn
                    .extend(selected.iter().map(|attack| attack.attacker));
            }
        }
        let mut records = vec![RulesRecord::Event(event)];
        records.append(&mut discoveries);
        Ok(RulesTransition::new(self, id, RulesJournal { records }))
    }
}

fn additional_trigger_count(
    ability: &TriggeredAbility,
    source: &SupportedAppearance,
    controller: SeatId,
    causes: &TriggerCauseSet,
    before: &RulesView<'_>,
    after: &RulesView<'_>,
) -> usize {
    use crate::characteristics::{
        StaticContinuousAbility, StaticModifierRule, TriggerMultiplierEvent,
    };
    if source.object.zone() != ObjectZone::Battlefield {
        return 0;
    }
    let cut = if ability.condition.is_lookback() {
        before
    } else {
        after
    };
    let is_sacrifice_trigger = matches!(
        ability.condition.event,
        TriggerEvent::PlayerAction {
            action: PlayerTriggerAction::Sacrificed(_),
            ..
        }
    );
    let is_creature_death = !is_sacrifice_trigger
        && causes.members().iter().any(|member| {
            matches!(
                &member.related,
                TriggerRelated::ZoneChange { before, after }
                    if before.object.zone() == ObjectZone::Battlefield
                        && before.is_creature()
                        && matches!(after.object.zone(), ObjectZone::Graveyard(_))
            )
        });
    fn check_ev(
        ev: &TriggerMultiplierEvent,
        causes: &TriggerCauseSet,
        cut: &RulesView<'_>,
    ) -> bool {
        let excluded = BTreeSet::new();
        match ev {
            TriggerMultiplierEvent::Any => true,
            TriggerMultiplierEvent::Enters(filter) => causes.members().iter().any(|m| {
                matches!(
                    &m.related,
                    TriggerRelated::ZoneChange { before: b, after: a }
                        if b.object.zone() != ObjectZone::Battlefield
                            && a.object.zone() == ObjectZone::Battlefield
                            && cut.evaluated.get(a.object.id()).is_some_and(|view| {
                                filter.matches_static_characteristics(
                                    view,
                                    &excluded,
                                    || Some(view.defined.mana_value.clone()),
                                )
                            })
                )
            }),
            TriggerMultiplierEvent::Dies(filter) => causes.members().iter().any(|m| {
                matches!(
                    &m.related,
                    TriggerRelated::ZoneChange { before: b, after: a }
                        if b.object.zone() == ObjectZone::Battlefield
                            && matches!(a.object.zone(), ObjectZone::Graveyard(_))
                            && cut.evaluated.get(b.object.id()).is_some_and(|view| {
                                filter.matches_static_characteristics(
                                    view,
                                    &excluded,
                                    || Some(view.defined.mana_value.clone()),
                                )
                            })
                )
            }),
            TriggerMultiplierEvent::LeavesBattlefield(filter) => causes.members().iter().any(|m| {
                matches!(
                    &m.related,
                    TriggerRelated::ZoneChange { before: b, after: a }
                        if b.object.zone() == ObjectZone::Battlefield
                            && a.object.zone() != ObjectZone::Battlefield
                            && cut.evaluated.get(b.object.id()).is_some_and(|view| {
                                filter.matches_static_characteristics(
                                    view,
                                    &excluded,
                                    || Some(view.defined.mana_value.clone()),
                                )
                            })
                )
            }),
            TriggerMultiplierEvent::Attacks(filter) => causes.members().iter().any(|m| {
                let attacker_ids: Vec<ObjectId> = match &m.related {
                    TriggerRelated::Combatant(id) | TriggerRelated::AttackedAlone(id) => vec![*id],
                    TriggerRelated::AttackGroup { creatures, .. }
                    | TriggerRelated::DeclaredSubtypeAttackers(creatures) => creatures.clone(),
                    _ => Vec::new(),
                };
                attacker_ids.iter().any(|id| {
                    cut.evaluated.get(*id).is_some_and(|view| {
                        filter.matches_static_characteristics(view, &excluded, || {
                            Some(view.defined.mana_value.clone())
                        })
                    })
                })
            }),
            TriggerMultiplierEvent::SpellCastOrCopy(spell_filter) => {
                causes.members().iter().any(|m| match &m.related {
                    TriggerRelated::SpellCast(cast) => {
                        spell_filter.matches_spell_appearance(cast.spell())
                    }
                    TriggerRelated::SpellCopied(_) => true,
                    _ => false,
                })
            }
            TriggerMultiplierEvent::DealsDamage {
                combat_only,
                to_player_only,
            } => causes.members().iter().any(|m| match &m.related {
                TriggerRelated::CombatDamageToPlayer { .. }
                | TriggerRelated::SubtypeCombatDamage { .. } => true,
                TriggerRelated::CombatDamageToPlaneswalker { .. } => !*to_player_only,
                TriggerRelated::DamageToOpponent { .. } => !*combat_only,
                TriggerRelated::DamageDealtToPermanent { .. }
                | TriggerRelated::AttachedCreatureDamaged { .. }
                | TriggerRelated::DamageReceived { .. } => !*combat_only && !*to_player_only,
                _ => false,
            }),
            TriggerMultiplierEvent::AnyOf(items) => {
                items.iter().any(|item| check_ev(item, causes, cut))
            }
        }
    }
    let mut total = 0usize;
    for permanent in cut
        .evaluated
        .objects
        .values()
        .filter(|o| o.zone == ObjectZone::Battlefield && o.controller == Some(controller))
    {
        for rule in permanent
            .printed_text
            .continuous
            .iter()
            .chain(&permanent.printed_text.additional_battlefield_abilities)
        {
            if !(permanent.abilities.continuous == Some(rule.id())
                || permanent
                    .abilities
                    .additional_battlefield_abilities
                    .contains(&rule.id()))
            {
                continue;
            }
            match rule {
                StaticContinuousAbility::Modifier {
                    rule: StaticModifierRule::AdditionalCreatureDeathTrigger,
                    ..
                } => {
                    if is_creature_death {
                        total = total.saturating_add(1);
                    }
                }
                StaticContinuousAbility::Modifier {
                    rule: StaticModifierRule::AdditionalTrigger(mult),
                    ..
                } => {
                    if mult.exclude_self && permanent.object == source.object.id() {
                        continue;
                    }
                    if mult.exclude_sacrifice_triggers && is_sacrifice_trigger {
                        continue;
                    }
                    if let Some(sel) = &mult.triggering_source
                        && !crate::characteristics::selector_matches_permanent(
                            cut.cards,
                            &cut.evaluated,
                            permanent.object,
                            controller,
                            sel,
                            source.object.id(),
                            cut.name_rules,
                        )
                    {
                        continue;
                    }
                    if check_ev(&mult.event, causes, cut) {
                        total = total.saturating_add(usize::from(mult.additional));
                    }
                }
                _ => {}
            }
        }
    }
    for (owner, rule) in &cut.evaluated.emblem_rules {
        if *owner != controller {
            continue;
        }
        match rule {
            StaticContinuousAbility::Modifier {
                rule: StaticModifierRule::AdditionalCreatureDeathTrigger,
                ..
            } => {
                if is_creature_death {
                    total = total.saturating_add(1);
                }
            }
            StaticContinuousAbility::Modifier {
                rule: StaticModifierRule::AdditionalTrigger(mult),
                ..
            } => {
                if mult.exclude_sacrifice_triggers && is_sacrifice_trigger {
                    continue;
                }
                if let Some(sel) = &mult.triggering_source
                    && !crate::characteristics::selector_matches_permanent(
                        cut.cards,
                        &cut.evaluated,
                        ObjectId::SENTINEL,
                        controller,
                        sel,
                        source.object.id(),
                        cut.name_rules,
                    )
                {
                    continue;
                }
                if check_ev(&mult.event, causes, cut) {
                    total = total.saturating_add(usize::from(mult.additional));
                }
            }
            _ => {}
        }
    }
    total
}
