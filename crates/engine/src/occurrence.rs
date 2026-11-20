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
        CreatureTypeSlot, ExecutableBody, ObjectRelation, OrdinaryCondition, PlayerRelation,
        TriggeredAbility, TriggeredAbilityId,
    },
    shuffle::ShuffleRecord,
    stack_window::{SpellCopyObject, StackEntryId, StackKind, StackWindow},
    targets::TargetChoice,
    turn::{TurnPosition, TurnScheduler},
};
use spellwake_contracts::{RulesInteger, creature_types::CreatureTypeId};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

mod causes;
pub use causes::{TriggerCauseKey, TriggerCauseMember, TriggerCauseSet};

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
    pub object: GameObject,
    pub copiable_names: spellwake_contracts::names::ObjectNames,
    pub current_names: spellwake_contracts::names::ObjectNames,
    pub copiable_supertypes: BTreeSet<crate::program::Supertype>,
    pub current_supertypes: BTreeSet<crate::program::Supertype>,
    pub copiable_card_types: BTreeSet<CardType>,
    pub copiable_creature_subtypes: BTreeSet<CreatureTypeId>,
    pub copiable_land_subtypes: BTreeSet<crate::land::LandType>,
    pub types: BTreeSet<CardType>,
    pub creature_subtypes: BTreeSet<CreatureTypeId>,
    pub land_subtypes: BTreeSet<crate::land::LandType>,
    pub intrinsic_mana: BTreeSet<crate::land::BasicLandType>,
    pub colors: BTreeSet<Color>,
    pub abilities: PrintedAbilityRoster,
    /// Layer-3 printed definitions at this exact cut, including removed printed
    /// abilities; the separate roster determines which ones actually exist.
    pub printed_text: PrintedText,
    /// Current granted keywords from the same evaluated world as types/abilities.
    pub keywords: BTreeSet<KeywordAbility>,
    pub copiable_power_toughness: Option<PrintedPowerToughness>,
    pub battlefield_counters: Option<PowerToughnessCounters>,
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
    pub fn is_creature(&self) -> bool {
        self.types.contains(&CardType::Creature)
    }
}

/// One checked physical spell at its actual completed casting cut (CR 601.2i).
/// The immutable capture survives later countering, resolution or source departure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CastOccurrence {
    entry: StackEntryId,
    controller: SeatId,
    spell: SupportedAppearance,
}
impl CastOccurrence {
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
    ZeroToughness,
    LethalDestruction,
    Legend,
    World,
}
impl StateBasedMoveReason {
    const fn bit(self) -> u16 {
        match self {
            Self::ZeroToughness => 1,
            Self::LethalDestruction => 2,
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
            StateBasedMoveReason::LethalDestruction,
            StateBasedMoveReason::Legend,
            StateBasedMoveReason::World,
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
    /// CR 800.4c: the last surviving control effect ended after default control left.
    DefaultControllerDeparted,
    /// CR 800.4a exile after owned objects and control effects leave.
    PlayerDeparture,
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
    ReturnToOwnersHand,
    Countered,
    SacrificeCost,
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
    pub before: SupportedAppearance,
    pub after: SupportedAppearance,
    pub reason: ZoneReason,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatusReason {
    UntapTurn,
    ManaAbilityCost,
    ActivatedAbilityCost,
    AttackDeclaration,
    MechanismUnit,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TriggerRelated {
    /// A phased-in permanent left the game; outside the game is not a zone.
    LeftGame {
        before: SupportedAppearance,
    },
    LibraryAction(crate::effects::LibraryAction),
    /// Own the complete causing spell, not a later lookup or a printed-name alias.
    SpellCast(Box<CastOccurrence>),
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
    NonpositiveLife,
    FailedDraw,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryCompletion {
    Resolved,
    AllTargetsIllegal,
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
    PlayerDeparture {
        players: BTreeSet<SeatId>,
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
    DamageAdjusted {
        batch: DamageBatchId,
    },
    DamageDealt {
        batch: DamageBatchId,
    },
    DamageResults {
        batch: DamageBatchId,
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
    StepBegan {
        position: TurnPosition,
    },
    MechanismUnit,
}
/// One canonical occurrence. Overlapping move properties share its one identity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RulesOccurrence {
    /// CR 603.6c lookback evidence, without a fabricated destination or death.
    ObjectLeftGame {
        before: Box<SupportedAppearance>,
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
    ManaPoolEmptied {
        player: SeatId,
        previous: BTreeMap<ManaType, RulesInteger>,
    },
    CopyCreated {
        copiable_card_types: BTreeSet<CardType>,
        copiable_creature_subtypes: BTreeSet<CreatureTypeId>,
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
    ContinuousEffectCreated {
        effect: ResolvedContinuousEffect,
        appearances: Vec<CharacteristicAppearanceChange>,
    },
    CleanupCharacteristics {
        expired_effects: Vec<ContinuousEffectId>,
        marks_removed: Vec<MarkedDamageRemoval>,
        appearances: Vec<CharacteristicAppearanceChange>,
    },
    CombatantsRemoved {
        participants: Vec<Combatant>,
    },
    CountersPlaced {
        object: ObjectId,
        before: PowerToughnessCounters,
        added: PowerToughnessCounters,
        after: PowerToughnessCounters,
        before_appearance: Box<SupportedAppearance>,
        after_appearance: Box<SupportedAppearance>,
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
    DamageMultiplied(DamageMultiplication),
    DamageDealt {
        damage: AppliedDamage,
        kind: DamageKind,
        origin: DamageOrigin,
    },
    DamageResult {
        recipient: DamageRecipient,
        before: RulesInteger,
        after: RulesInteger,
        components: Vec<DamageAttemptId>,
    },
    PlayerLost {
        player: SeatId,
        reasons: BTreeSet<PlayerLossReason>,
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
    EntryCompleted {
        entry: StackEntryId,
        controller: SeatId,
        kind: StackKind,
        completion: EntryCompletion,
    },
    StepBegan {
        position: TurnPosition,
        active: Option<SeatId>,
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
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WaitingInstance {
    pub id: TriggerInstanceId,
    pub definition: TriggeredAbilityId,
    pub causes: TriggerCauseSet,
    pub source: SupportedAppearance,
    pub controller: SeatId,
    pub body: ExecutableBody,
}
/// Execution keeps no duplicate controller; the real StackEntry controls the body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TriggerBinding {
    pub instance: TriggerInstanceId,
    pub definition: TriggeredAbilityId,
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
    pub(crate) revision: u64,
    pub(crate) boundary: CheckBoundary,
    pub(crate) event: CheckEventId,
    pub(crate) wave: PlacementWaveId,
    pub(crate) chooser: SeatId,
    pub(crate) instance: TriggerInstanceId,
    pub(crate) entry: StackEntryId,
    pub(crate) domain: Vec<crate::targets::TargetRef>,
}
impl TriggerTargetChoice {
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
    /// CR 800.4g object choice reassignment; no Magic event or instruction restart.
    ChooserReplaced {
        entry: StackEntryId,
        instruction: usize,
        departed: SeatId,
        controller: SeatId,
        candidates: Vec<SeatId>,
        selected: SeatId,
    },
    /// One atomic CR 800.4a cut, distinct from state-based loss and zone changes.
    PlayersDeparted {
        players: BTreeSet<SeatId>,
        owned_objects: Vec<GameObject>,
        ended_control: Vec<ResolvedContinuousEffect>,
        ceased_entries: Vec<crate::stack_window::StackEntry>,
        ceased_copies: Vec<SpellCopyObject>,
        discarded_triggers: Vec<WaitingInstance>,
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
    /// Actual quoted resolution cost and explicit result; not a Magic event.
    ResolutionPaymentOutcome {
        entry: StackEntryId,
        instruction: usize,
        payer: SeatId,
        cost: crate::mana::ManaCost,
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
    state: S,
    result: R,
    append: RulesJournal,
}
impl<S, R> RulesTransition<S, R> {
    pub(crate) fn new(state: S, result: R, append: RulesJournal) -> Self {
        Self {
            state,
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
        self.state
    }
}

/// Complete relevant immutable queries at one actual cut, not a replayed journal world.
pub(crate) struct RulesView<'a> {
    pub(crate) window: &'a StackWindow,
    pub(crate) cards: &'a GameObjects,
    pub(crate) characteristics: &'a CharacteristicState,
    pub(crate) evaluated: Arc<EvaluatedCharacteristics>,
    pub(crate) marks: &'a BTreeMap<ObjectId, RulesInteger>,
    pub(crate) mana: &'a BTreeMap<(SeatId, ManaType), RulesInteger>,
    pub(crate) life: &'a LifeState,
    pub(crate) combat: Option<&'a CombatState>,
    pub(crate) turn: Option<&'a TurnScheduler>,
}
impl RulesView<'_> {
    /// Capture only the canonical physical spell in this actual completed world.
    ///
    /// # Errors
    /// Returns `InvalidWorld` for a missing/noncard entry, inconsistent identities
    /// or controllers, or unavailable evaluated physical Stack appearance.
    pub(crate) fn cast_occurrence(
        &self,
        entry: StackEntryId,
        controller: SeatId,
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
        Ok(CastOccurrence {
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
        Some(SupportedAppearance {
            object: physical.clone(),
            copiable_names: current.defined.names.clone(),
            current_names: current.current_names.clone(),
            copiable_supertypes: current.defined.supertypes.clone(),
            current_supertypes: current.current_supertypes.clone(),
            copiable_card_types: current.defined.types.clone(),
            copiable_creature_subtypes: current.defined.creature_subtypes.clone(),
            copiable_land_subtypes: current.defined.land_subtypes.clone(),
            types: current.current_types.clone(),
            creature_subtypes: current.current_creature_subtypes.clone(),
            land_subtypes: current.current_land_subtypes.clone(),
            intrinsic_mana: current.intrinsic_mana.clone(),
            colors: current.current_colors.clone(),
            abilities: current.abilities.clone(),
            printed_text: current.printed_text.clone(),
            keywords: current.keywords.clone(),
            copiable_power_toughness: current.defined.printed_pt.clone(),
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

/// Only checked counters and owned waiting instances survive acceptance.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TriggerState {
    next_event: u64,
    next_instance: u64,
    next_wave: u64,
    waiting: BTreeMap<TriggerInstanceId, WaitingInstance>,
}
impl Default for TriggerState {
    fn default() -> Self {
        Self {
            next_event: 1,
            next_instance: 1,
            next_wave: 1,
            waiting: BTreeMap::new(),
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
/// Test-only inconsistencies in a genuinely reached capture, not source producers.
#[cfg(test)]
#[derive(Debug, Clone)]
pub(crate) enum QualificationCaptureFault {
    Targetless,
    Source(GameObject),
}
impl TriggerState {
    #[cfg(test)]
    pub(crate) fn qualification_corrupt_capture(
        &mut self,
        id: TriggerInstanceId,
        fault: QualificationCaptureFault,
    ) {
        let waiting = self.waiting.get_mut(&id).expect("actual captured instance");
        match fault {
            QualificationCaptureFault::Targetless => {
                waiting.body.target = crate::program::BodyTarget::None
            }
            QualificationCaptureFault::Source(object) => waiting.source.object = object,
        }
    }

    pub(crate) fn remove_departed(&mut self, players: &BTreeSet<SeatId>) -> Vec<WaitingInstance> {
        let mut removed = Vec::new();
        self.waiting.retain(|_, instance| {
            if players.contains(&instance.controller) {
                removed.push(instance.clone());
                false
            } else {
                true
            }
        });
        removed
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
        let mut sources = BTreeMap::new();
        for (view, lookback) in [(before, true), (after, false)] {
            for object in view.cards.iter().filter(|object| {
                matches!(object.zone(), ObjectZone::Battlefield | ObjectZone::Stack)
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
                if current.abilities.triggered.is_empty() {
                    continue;
                }
                let mut abilities = Vec::new();
                for definition in &current.abilities.triggered {
                    let ability = current
                        .printed_text
                        .triggered
                        .iter()
                        .find(|ability| ability.id == *definition)
                        .ok_or(TriggerError::InvalidWorld)?;
                    if functions_in_zone(ability.condition, object.zone())
                        && is_lookback(ability.condition) == lookback
                    {
                        abilities.push(ability);
                    }
                }
                if abilities.is_empty() {
                    continue;
                }
                let source = view
                    .appearance(object.id())
                    .ok_or(TriggerError::InvalidWorld)?;
                let controller = current.controller.ok_or(TriggerError::InvalidWorld)?;
                for ability in abilities {
                    sources.insert(
                        (object.id(), ability.id),
                        (source.clone(), controller, ability),
                    );
                }
            }
        }
        for (_, (source, controller, ability)) in sources {
            // CR 800.4d: a departing player's lookback sources may exist in
            // the predecessor snapshot, but cannot create new waiting work.
            if !after.window.participants().contains(&controller) {
                continue;
            }
            let mut members = Vec::new();
            members
                .try_reserve(event.occurrences.len())
                .map_err(|_| TriggerError::TriggerCauseCapacity)?;
            for occurrence in &event.occurrences {
                if let Some((player, related)) =
                    match_related(ability, &source, controller, &occurrence.value, after)?
                {
                    members.push(TriggerCauseMember {
                        occurrence: occurrence.id,
                        related_player: Some(player),
                        related,
                    });
                }
            }
            for causes in causes::group_matches(id, ability.grouping, members)? {
                let next = self
                    .next_instance
                    .checked_add(1)
                    .ok_or(TriggerError::InstanceIdentityExhausted)?;
                let instance = WaitingInstance {
                    id: TriggerInstanceId(self.next_instance),
                    definition: ability.id,
                    causes,
                    source: source.clone(),
                    controller,
                    body: ability.body.clone(),
                };
                self.next_instance = next;
                self.waiting.insert(instance.id, instance.clone());
                discoveries.push(RulesRecord::TriggerDiscovered(Box::new(instance)));
            }
        }
        let mut records = vec![RulesRecord::Event(event)];
        records.append(&mut discoveries);
        Ok(RulesTransition::new(self, id, RulesJournal { records }))
    }
}
fn is_lookback(condition: OrdinaryCondition) -> bool {
    match condition {
        OrdinaryCondition::LeavesBattlefield(_) | OrdinaryCondition::Dies(_) => true,
        OrdinaryCondition::Scried(_)
        | OrdinaryCondition::Surveilled(_)
        | OrdinaryCondition::SelfCast
        | OrdinaryCondition::SpellCast { .. }
        | OrdinaryCondition::TokenCreated(_)
        | OrdinaryCondition::LifeChanged { .. }
        | OrdinaryCondition::Enters(_)
        | OrdinaryCondition::BecomesTapped(_)
        | OrdinaryCondition::Discarded(_)
        | OrdinaryCondition::StepBegan { .. } => false,
    }
}
/// Represented function zones only; every added condition must choose its policy.
fn functions_in_zone(condition: OrdinaryCondition, zone: ObjectZone) -> bool {
    match condition {
        OrdinaryCondition::SelfCast => zone == ObjectZone::Stack,
        OrdinaryCondition::Scried(_)
        | OrdinaryCondition::Surveilled(_)
        | OrdinaryCondition::SpellCast { .. }
        | OrdinaryCondition::TokenCreated(_)
        | OrdinaryCondition::LifeChanged { .. }
        | OrdinaryCondition::Enters(_)
        | OrdinaryCondition::LeavesBattlefield(_)
        | OrdinaryCondition::Dies(_)
        | OrdinaryCondition::BecomesTapped(_)
        | OrdinaryCondition::Discarded(_)
        | OrdinaryCondition::StepBegan { .. } => zone == ObjectZone::Battlefield,
    }
}
fn object_matches(
    relation: ObjectRelation,
    source: &SupportedAppearance,
    controller: SeatId,
    object: &SupportedAppearance,
) -> bool {
    let same = source.object.id() == object.object.id();
    match relation {
        ObjectRelation::SelfObject => same,
        ObjectRelation::AnyCreature => object.is_creature(),
        ObjectRelation::AnotherCreature => object.is_creature() && !same,
        ObjectRelation::ControlledCreature => {
            object.is_creature() && object.object.controller() == Some(controller)
        }
        ObjectRelation::AnotherControlledCreature => {
            object.is_creature() && !same && object.object.controller() == Some(controller)
        }
    }
}
fn player_matches(
    relation: PlayerRelation,
    controller: SeatId,
    player: SeatId,
    view: &RulesView<'_>,
) -> bool {
    view.window.participants().contains(&player)
        && match relation {
            PlayerRelation::Controller => player == controller,
            PlayerRelation::Opponent => player != controller,
            PlayerRelation::AnyParticipant => true,
        }
}
fn match_related(
    ability: &TriggeredAbility,
    source: &SupportedAppearance,
    controller: SeatId,
    value: &RulesOccurrence,
    after: &RulesView<'_>,
) -> Result<Option<(SeatId, TriggerRelated)>, TriggerError> {
    if let RulesOccurrence::CastCompleted(cast) = value {
        return match_cast(ability.condition, source, controller, cast, after);
    }
    if let OrdinaryCondition::Enters(relation) = ability.condition {
        let object = match value {
            RulesOccurrence::ZoneChange(change)
                if change.after.object.zone() == ObjectZone::Battlefield
                    && change.before.object.zone() != ObjectZone::Battlefield =>
            {
                &change.after
            }
            RulesOccurrence::TokenCreated { after, .. } => after.as_ref(),
            _ => return Ok(None),
        };
        if !object_matches(relation, source, controller, object) {
            return Ok(None);
        }
        let player = object
            .object
            .controller()
            .ok_or(TriggerError::InvalidWorld)?;
        return Ok(match value {
            RulesOccurrence::ZoneChange(change) => Some((
                player,
                TriggerRelated::ZoneChange {
                    before: change.before.clone(),
                    after: change.after.clone(),
                },
            )),
            RulesOccurrence::TokenCreated { creator, after } => Some((
                player,
                TriggerRelated::TokenCreated {
                    creator: *creator,
                    after: after.as_ref().clone(),
                },
            )),
            _ => None,
        });
    }
    Ok(match (ability.condition, value) {
        (
            OrdinaryCondition::TokenCreated(player),
            RulesOccurrence::TokenCreated {
                creator,
                after: appearance,
            },
        ) if player_matches(player, controller, *creator, after) => Some((
            *creator,
            TriggerRelated::TokenCreated {
                creator: *creator,
                after: appearance.as_ref().clone(),
            },
        )),
        (OrdinaryCondition::Scried(player), RulesOccurrence::LibraryAction(action))
        | (OrdinaryCondition::Surveilled(player), RulesOccurrence::LibraryAction(action))
            if action.requested.is_positive()
                && ((matches!(ability.condition, OrdinaryCondition::Scried(_))
                    && action.kind == crate::effects::LibraryActionKind::Scry)
                    || (matches!(ability.condition, OrdinaryCondition::Surveilled(_))
                        && action.kind == crate::effects::LibraryActionKind::Surveil))
                && player_matches(player, controller, action.player, after) =>
        {
            Some((action.player, TriggerRelated::LibraryAction(action.clone())))
        }
        (OrdinaryCondition::LifeChanged { kind, player }, RulesOccurrence::LifeChanged(change))
            if change.kind == kind && player_matches(player, controller, change.player, after) =>
        {
            Some((change.player, TriggerRelated::LifeChanged(change.clone())))
        }
        (
            OrdinaryCondition::LeavesBattlefield(relation),
            RulesOccurrence::ObjectLeftGame { before },
        ) if before.object.zone() == ObjectZone::Battlefield
            && object_matches(relation, source, controller, before) =>
        {
            Some((
                before
                    .object
                    .controller()
                    .ok_or(TriggerError::InvalidWorld)?,
                TriggerRelated::LeftGame {
                    before: before.as_ref().clone(),
                },
            ))
        }
        (OrdinaryCondition::LeavesBattlefield(relation), RulesOccurrence::ZoneChange(change))
            if change.before.object.zone() == ObjectZone::Battlefield
                && change.after.object.zone() != ObjectZone::Battlefield
                && object_matches(relation, source, controller, &change.before) =>
        {
            Some((
                change
                    .before
                    .object
                    .controller()
                    .ok_or(TriggerError::InvalidWorld)?,
                TriggerRelated::ZoneChange {
                    before: change.before.clone(),
                    after: change.after.clone(),
                },
            ))
        }
        (OrdinaryCondition::Dies(relation), RulesOccurrence::ZoneChange(change))
            if change.before.object.zone() == ObjectZone::Battlefield
                && matches!(change.after.object.zone(), ObjectZone::Graveyard(_))
                && change.before.is_creature()
                && object_matches(relation, source, controller, &change.before) =>
        {
            Some((
                change
                    .before
                    .object
                    .controller()
                    .ok_or(TriggerError::InvalidWorld)?,
                TriggerRelated::ZoneChange {
                    before: change.before.clone(),
                    after: change.after.clone(),
                },
            ))
        }
        (
            OrdinaryCondition::BecomesTapped(relation),
            RulesOccurrence::StatusChange { change, .. },
        ) if change.before.zone() == ObjectZone::Battlefield
            && change.after.zone() == ObjectZone::Battlefield
            && change.before.id() == change.after.id()
            && change.before.tapped() == Some(false)
            && change.after.tapped() == Some(true) =>
        {
            let object = after
                .appearance(change.after.id())
                .ok_or(TriggerError::InvalidWorld)?;
            if object_matches(relation, source, controller, &object) {
                Some((
                    object
                        .object
                        .controller()
                        .ok_or(TriggerError::InvalidWorld)?,
                    TriggerRelated::StatusChange(change.clone()),
                ))
            } else {
                None
            }
        }
        (OrdinaryCondition::Discarded(relation), RulesOccurrence::ZoneChange(change)) => {
            let ZoneReason::Discard { player } = change.reason else {
                return Ok(None);
            };
            player_matches(relation, controller, player, after).then(|| {
                (
                    player,
                    TriggerRelated::Discard {
                        player,
                        before: change.before.clone(),
                        after: change.after.clone(),
                    },
                )
            })
        }
        (
            OrdinaryCondition::StepBegan { step, player },
            RulesOccurrence::StepBegan {
                position,
                active: Some(active),
            },
        ) if position.step == step && player_matches(player, controller, *active, after) => Some((
            *active,
            TriggerRelated::Step {
                position: *position,
                active: *active,
            },
        )),
        _ => None,
    })
}

fn match_cast(
    condition: OrdinaryCondition,
    source: &SupportedAppearance,
    controller: SeatId,
    cast: &CastOccurrence,
    after: &RulesView<'_>,
) -> Result<Option<(SeatId, TriggerRelated)>, TriggerError> {
    let matched = match condition {
        OrdinaryCondition::SelfCast => {
            let same = source.object.id() == cast.spell().object.id();
            if same && controller != cast.controller() {
                return Err(TriggerError::InvalidWorld);
            }
            same
        }
        OrdinaryCondition::SpellCast { player, filter } => {
            player_matches(player, controller, cast.controller(), after)
                && filter.matches(&cast.spell().types)
        }
        _ => return Ok(None),
    };
    if !after.window.participants().contains(&cast.controller()) {
        return Err(TriggerError::InvalidWorld);
    }
    Ok(matched.then(|| {
        (
            cast.controller(),
            TriggerRelated::SpellCast(Box::new(cast.clone())),
        )
    }))
}
