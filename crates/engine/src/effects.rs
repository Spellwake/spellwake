//! Native kernel for finite authored definitions and exact-cost ordered bodies.
//! Control, semantic text, types, colors, ability removal/grants, defining
//! characteristics and PT share one pure characteristic evaluator. Resolution
//! captures face-up permanent-copy definitions through the same effect owner.
//! Full card metadata, entry/token/static copies, general linked/granted
//! abilities, replacement language and complete formats remain unimplemented.
//! Mana-only, tap and sacrifice activations share payment, targets and payloads.
//! GameState delegates opening, turns, draw, combat, checks and cleanup here;
//! actual operation cuts publish occurrences before normal check handoffs.
//! This is not the general Magic interpreter or a complete-card qualification.
#[cfg(test)]
mod characteristic_observations;
#[cfg(test)]
pub(crate) mod characteristic_reference_capture;
#[cfg(test)]
mod characteristics_remaining_tests;
#[cfg(test)]
mod characteristics_tests;
mod check_flow;
mod combat_damage;
mod combat_flow;
mod control_flow;
mod creature_type_flow;
mod damage_flow;
mod departure;
mod library_top;
mod life_flow;
use library_top::LibraryTopWork;
pub use library_top::{
    LibraryAction, LibraryActionKind, LibraryChoiceKind, LibraryPublicMove, LibraryTopFrame,
    LibraryTopReply,
};
mod lifecycle_flow;
#[cfg(test)]
mod names_snapshot_tests;
mod occurrence_flow;
mod opening_capture;
mod resolution_choices;
mod resolution_selection;
mod zone_replacement;
use resolution_choices::{ManaColorWork, ManaParent, OptionalEffectWork, ResolutionPaymentWork};
pub use resolution_choices::{
    OptionalEffectChoice, OptionalEffectFrame, PendingWorkKind, ReplacementChooserFrame,
    ResolutionChoiceState, ResolutionPaymentFrame, ResolutionPaymentOutcome,
    ResolutionPaymentReply,
};
use resolution_selection::ResolutionSelectionWork;
pub use resolution_selection::{
    ResolutionAcceptedGraveyardOrder, ResolutionAcceptedSelection, ResolutionGraveyardGroup,
    ResolutionGraveyardOrderFrame, ResolutionSelectedObject, ResolutionSelectionBatch,
    ResolutionSelectionCertificate, ResolutionSelectionDepartureState, ResolutionSelectionFrame,
    ResolutionSelectionKind, ResolutionSelectionOperation, ResolutionSelectionOrderCertificate,
    ResolutionSelectionRequirement, ResolutionSelectionStage, ResolutionSelectionState,
};
pub use zone_replacement::*;
#[cfg(test)]
pub(crate) mod private_qualification;
#[cfg(test)]
mod supertype_projection_tests;
mod token_entry_flow;
#[cfg(test)]
mod transport_tests;
#[cfg(test)]
mod world_history_tests;
use crate::activated::{AbilityActivationRecord, AbilityActivationTransition, ActivatedAbilityId};
#[cfg(test)]
use crate::characteristics::PowerToughness;
use crate::checks::{CheckCompletion, CheckDecision, CheckError, CheckJournal};
use crate::combat::CombatRules;
use crate::combat::{
    AttackDeclaration, BlockDeclaration, CombatAssignmentRecord, CombatCursor,
    CombatDamageAssignment, CombatDecision, CombatDeclarationRecord, CombatError, CombatState,
};
#[cfg(test)]
use crate::mana::ManaAbilityId;
use crate::occurrence::{
    DrawOrigin, ExecutionBindings, RulesJournal, RulesTransition, TriggerInstanceId, TriggerOrder,
    TriggerTargetChoice, WaitingInstance,
};
use crate::setup::GameSeed;
use crate::token::{TokenBatchMember, TokenTimestampOrder};
use crate::{
    TargetId,
    actions::{LandPlayRecord, PermanentEntryRecord, TimingError, validate_main_phase},
    characteristics::{
        CharacteristicInvariantError, CharacteristicState, CharacteristicTimestamp,
        CharacteristicWorld, CharacteristicsError, ContinuousEffectId, EvaluatedCharacteristics,
        KeywordAbility, ObjectCharacteristics, PowerToughnessCounters, PowerToughnessEvaluation,
        ResolvedContinuousEffect, evaluate,
    },
    damage::{
        ActiveDamageBatch, DamageAllocation, DamageApplication, DamageAttemptId, DamageBatchId,
        DamageBatchPhase, DamageEffectCreation, DamageEffectId, DamageEffectTarget, DamageError,
        DamageInput, DamageJournal, DamageKind, DamageMultiplier, DamageMultiplierId, DamageOrigin,
        DamageRecipient, DamageScope, DamageSource, DamageSourceFilter, DamageState,
        PreventionShield, PreventionShieldId,
    },
    draw::{DrawEvent, DrawJournal},
    mana::{
        ManaCost, ManaOutput, ManaPayment, ManaPaymentError, ManaPaymentRecord,
        ManaProductionRecord, ManaSpend, ManaType, prepare_payment,
    },
    object_identity::{CardId, GameObject, GameObjects, ObjectDestination, ObjectId, ObjectZone},
    priority::{PriorityPhase, SeatId},
    stack_window::{StackEntryId, StackKind, StackWindow, StackWindowError, WindowPhase},
    targets::{
        ResolutionPermission, TargetCheck, TargetChoice, TargetClause, TargetClauseId, TargetRef,
    },
    turn::{TurnScheduler, TurnWork, WorkCompleted},
};
#[cfg(test)]
use crate::{
    activated::ActivatedAbility,
    mana::{FloatingMana, ManaAbility},
};
use crate::{
    opening::MulliganChoice,
    shuffle::{EntropyTape, ShuffleRecord},
};
use check_flow::CheckProcess;
use damage_flow::DamageProgressStep;
#[cfg(test)]
pub(crate) use damage_flow::n04_player_batch_fixture;
use occurrence_flow::{
    characteristic_appearance_changes, desired_controllers, exit_occurrence_at, zone_occurrence_at,
};
use spellwake_contracts::{
    RulesInteger,
    creature_types::{CreatureTypeId, CreatureTypeVocabulary, CreatureVocabularyId},
};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;
use token_entry_flow::TokenEntryWork;

pub use crate::program::{
    BodyTarget, ExecutableBody, Instruction, Program, ProgramKind, SpellTargetFilter,
};
use crate::program::{CardType, CreatureTypeSlot, InstructionAmount};
/// One trusted resolution-time choice: retaining is distinct from fresh capture.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CopyTargetChoice {
    Retain,
    Change(TargetRef),
}
/// Owned post-target-check continuation. Only the executor constructs this frame.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CopyResolution {
    pub(crate) revision: u64,
    entry: StackEntryId,
    original: StackEntryId,
    chooser: SeatId,
    instruction: usize,
}
impl CopyResolution {
    pub fn instruction_index(&self) -> usize {
        self.instruction
    }
    pub fn entry(&self) -> StackEntryId {
        self.entry
    }
    pub fn original(&self) -> StackEntryId {
        self.original
    }
    pub fn chooser(&self) -> SeatId {
        self.chooser
    }
}
/// Readonly rule-wide application domain into the sole owned batch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DamageOrderChoice {
    pub(crate) revision: u64,
    batch: DamageBatchId,
    stage: u64,
    origin: DamageOrigin,
    chooser: SeatId,
    applications: Vec<DamageApplication>,
}
impl DamageOrderChoice {
    pub fn revision(&self) -> u64 {
        self.revision
    }
    pub fn batch(&self) -> DamageBatchId {
        self.batch
    }
    pub fn stage(&self) -> u64 {
        self.stage
    }
    pub fn origin(&self) -> &DamageOrigin {
        &self.origin
    }
    pub fn chooser(&self) -> SeatId {
        self.chooser
    }
    pub fn applications(&self) -> &[DamageApplication] {
        &self.applications
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DamageAllocationDomain {
    attempt: DamageAttemptId,
    source: DamageSource,
    amount: RulesInteger,
}
impl DamageAllocationDomain {
    pub fn attempt(&self) -> DamageAttemptId {
        self.attempt
    }
    pub fn source(&self) -> &DamageSource {
        &self.source
    }
    pub fn amount(&self) -> &RulesInteger {
        &self.amount
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreventionAllocationChoice {
    pub(crate) revision: u64,
    batch: DamageBatchId,
    stage: u64,
    origin: DamageOrigin,
    chooser: SeatId,
    application: DamageApplication,
    capacity: RulesInteger,
    required: RulesInteger,
    eligible: Vec<DamageAllocationDomain>,
}
impl PreventionAllocationChoice {
    pub fn revision(&self) -> u64 {
        self.revision
    }
    pub fn batch(&self) -> DamageBatchId {
        self.batch
    }
    pub fn stage(&self) -> u64 {
        self.stage
    }
    pub fn origin(&self) -> &DamageOrigin {
        &self.origin
    }
    pub fn chooser(&self) -> SeatId {
        self.chooser
    }
    pub fn application(&self) -> DamageApplication {
        self.application
    }
    pub fn capacity(&self) -> &RulesInteger {
        &self.capacity
    }
    pub fn required(&self) -> &RulesInteger {
        &self.required
    }
    pub fn eligible(&self) -> &[DamageAllocationDomain] {
        &self.eligible
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DamageDecision {
    Order(DamageOrderChoice),
    Allocation(PreventionAllocationChoice),
}
impl DamageDecision {
    pub fn revision(&self) -> u64 {
        match self {
            Self::Order(f) => f.revision(),
            Self::Allocation(f) => f.revision(),
        }
    }
    pub fn batch(&self) -> DamageBatchId {
        match self {
            Self::Order(f) => f.batch(),
            Self::Allocation(f) => f.batch(),
        }
    }
    pub fn stage(&self) -> u64 {
        match self {
            Self::Order(f) => f.stage(),
            Self::Allocation(f) => f.stage(),
        }
    }
    pub fn origin(&self) -> &DamageOrigin {
        match self {
            Self::Order(f) => f.origin(),
            Self::Allocation(f) => f.origin(),
        }
    }
    pub fn chooser(&self) -> SeatId {
        match self {
            Self::Order(f) => f.chooser(),
            Self::Allocation(f) => f.chooser(),
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EffectDecision {
    /// The surviving object controller selects a replacement for an absent chooser.
    ReplacementChooser(ReplacementChooserFrame),
    /// Exact precommit selected-effect replacement request.
    ZoneReplacement(ZoneReplacementFrame),
    LibraryTop(LibraryTopFrame),
    /// Executor-issued exact affected-player selection, potentially hidden.
    ResolutionSelect(ResolutionSelectionFrame),
    /// Executor-issued exact physical owner permutation, after every selection.
    ResolutionGraveyardOrder(ResolutionGraveyardOrderFrame),
    OptionalEffect(OptionalEffectFrame),
    ResolutionPayment(ResolutionPaymentFrame),
    TimestampOrder(TokenTimestampOrder),
    Checks(CheckDecision),
    Triggers(TriggerOrder),
    TriggerTarget(TriggerTargetChoice),
    CopyTargets(CopyResolution),
    Damage(DamageDecision),
    Combat(CombatDecision),
    CreatureType(CreatureTypeChoice),
}
impl EffectDecision {
    pub fn chooser(&self) -> SeatId {
        match self {
            Self::ReplacementChooser(f) => f.chooser(),
            Self::ZoneReplacement(f) => f.chooser(),
            Self::LibraryTop(f) => f.chooser(),
            Self::ResolutionSelect(f) => f.chooser(),
            Self::ResolutionGraveyardOrder(f) => f.chooser(),
            Self::OptionalEffect(f) => f.chooser(),
            Self::ResolutionPayment(f) => f.chooser(),
            Self::TimestampOrder(f) => f.chooser(),
            Self::Checks(f) => f.chooser(),
            Self::Triggers(f) => f.chooser(),
            Self::TriggerTarget(f) => f.chooser(),
            Self::CopyTargets(f) => f.chooser(),
            Self::Damage(f) => f.chooser(),
            Self::Combat(f) => f.chooser(),
            Self::CreatureType(f) => f.chooser(),
        }
    }
    fn required_choice_error(&self) -> EffectError {
        match self {
            Self::ReplacementChooser(_) => EffectError::ReplacementChooserRequired,
            Self::ZoneReplacement(_) => EffectError::SelectionRequired,
            Self::LibraryTop(_) => EffectError::LibraryTopChoiceRequired,
            Self::ResolutionSelect(_) | Self::ResolutionGraveyardOrder(_) => {
                EffectError::SelectionRequired
            }
            Self::OptionalEffect(_) => EffectError::OptionalEffectChoiceRequired,
            Self::ResolutionPayment(_) => EffectError::ResolutionPaymentChoiceRequired,
            Self::TimestampOrder(_) => EffectError::TimestampOrderRequired,
            Self::CopyTargets(_) => EffectError::CopyChoiceRequired,
            Self::TriggerTarget(_) => EffectError::TriggerTargetChoiceRequired,
            Self::CreatureType(_) => EffectError::CreatureTypeChoiceRequired,
            Self::Checks(_) | Self::Triggers(_) | Self::Damage(_) | Self::Combat(_) => {
                EffectError::DamageChoiceRequired
            }
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DecisionReply {
    /// One offered current participant, selected by the object controller.
    ReplacementChooser(SeatId),
    /// One answer to the current precommit replacement request.
    ZoneReplacement(ZoneReplacementReply),
    LibraryTop(LibraryTopReply),
    /// Distinct members of the current executor-issued frozen selection domain.
    SelectResolution(Vec<ObjectId>),
    /// Complete top-to-bottom permutation of the current physical owner domain.
    OrderResolutionGraveyard(Vec<ObjectId>),
    OptionalEffect(OptionalEffectChoice),
    ResolutionPayment(ResolutionPaymentReply),
    TimestampOrder(Vec<TokenBatchMember>),
    Discard(Vec<ObjectId>),
    OrderGraveyard(Vec<ObjectId>),
    /// Select one offered conflict and one exact member of its frozen domain.
    KeepLegend {
        conflict: crate::checks::LegendConflictId,
        keep: ObjectId,
    },
    OrderTriggers(Vec<TriggerInstanceId>),
    TriggerTarget(TargetRef),
    DeclareAttackers(Vec<AttackDeclaration>),
    DeclareBlockers(Vec<BlockDeclaration>),
    AssignCombatDamage(Vec<CombatDamageAssignment>),
    Mulligan(MulliganChoice),
    Bottom(Vec<ObjectId>),
    CopyTargets(CopyTargetChoice),
    DamageOrder(DamageApplication),
    AllocatePrevention(Vec<DamageAllocation>),
    ManaColor(ManaType),
    CreatureType(CreatureTypeId),
}
impl From<CopyTargetChoice> for DecisionReply {
    fn from(choice: CopyTargetChoice) -> Self {
        Self::CopyTargets(choice)
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuleCompletion {
    Checks(CheckCompletion),
    Resolution,
    TurnWork(WorkCompleted),
}
/// Operation diagnostics and continuation status; state/append belong to the envelope.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuleResult {
    Complete {
        checks: CheckJournal,
        draws: DrawJournal,
        completion: RuleCompletion,
        journal: DamageJournal,
        declarations: Vec<CombatDeclarationRecord>,
        assignments: Vec<CombatAssignmentRecord>,
        permanent_entry: Option<PermanentEntryRecord>,
    },
    Pending {
        checks: CheckJournal,
        draws: DrawJournal,
        frame: EffectDecision,
        journal: DamageJournal,
        declarations: Vec<CombatDeclarationRecord>,
        assignments: Vec<CombatAssignmentRecord>,
    },
}
/// The root consumes successor, typed result and current append together.
pub type RuleProgress = RulesTransition<EffectWindow, RuleResult>;
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SeedCard {
    pub card: CardId,
    pub owner: SeatId,
    pub destination: ObjectDestination,
    pub program: Program,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EffectError {
    /// The current optional owner first requires a replacement chooser selection.
    ReplacementChooserRequired,
    /// The supplied replacement token differs from the sole current owner.
    WrongReplacementChooserFrame,
    /// Departure requires an owner continuation not yet admitted by this path.
    UnsupportedDepartureState,
    /// A typed bounded replacement failure.
    ZoneReplacement(ZoneReplacementError),
    LibraryTopChoiceRequired,
    WrongLibraryTopFrame,
    InvalidLibraryPartition,
    InconsistentLibraryTopWork,
    /// A selected-object instruction is suspended at its exact native decision.
    SelectionRequired,
    /// The submitted frame differs from the current sole selection owner.
    WrongSelectionFrame,
    /// A selection does not have the executor's exact bounded cardinality.
    InvalidSelectionCount,
    /// A count-correct selection names the same incarnation more than once.
    DuplicateSelection,
    /// A selected incarnation is absent from the frozen current domain.
    SelectionOutsideDomain,
    /// An owner reply is not a complete distinct permutation of its domain.
    InvalidGraveyardPermutation,
    /// Internal stage, source, operation or complete-batch revalidation failed.
    InconsistentSelectionWork,
    OptionalEffectChoiceRequired,
    ResolutionPaymentChoiceRequired,
    WrongOptionalEffectFrame,
    WrongResolutionPaymentFrame,
    IllegalOptionalEffect,
    InvalidResolvingPlayer,
    TimestampOrderRequired,
    WrongTimestampOrderFrame,
    IllegalTimestampOrder,
    TokenCapacity,
    Trigger(crate::occurrence::TriggerError),
    Check(CheckError),
    Combat(CombatError),
    PlayerLossPending,
    Window(StackWindowError),
    InvalidSetup,
    UnknownObject,
    NotCastableSpell,
    /// CR205.4e: no current controlled legendary creature qualifies this cast.
    /// The not-yet-admitted planeswalker type family must extend this predicate.
    LegendarySpellPrerequisite,
    TargetRequired,
    UnexpectedTarget,
    NotLand,
    Timing(TimingError),
    NotAbilitySource,
    CreatureTapRestricted,
    UnknownActivatedAbility,
    NotManaSource,
    UnknownManaAbility,
    ManaColorChoiceRequired,
    WrongManaColorFrame,
    IllegalManaColor,
    NotOwnHand,
    IllegalTarget,
    InsufficientMana,
    ManaPaymentRequired,
    InvalidManaPayment(ManaPaymentError),
    WrongPhase,
    InconsistentProgram,
    CopyChoiceRequired,
    UnexpectedCopyChoice,
    WrongChooser,
    WrongResolutionFrame,
    WrongDamageFrame,
    Characteristics(CharacteristicsError),
    CharacteristicInvariant(CharacteristicInvariantError),
    Damage(DamageError),
    WrongDecisionKind,
    DamageChoiceRequired,
    TriggerTargetChoiceRequired,
    CreatureTypeChoiceRequired,
    WrongCreatureTypeFrame,
    IllegalCreatureType,
}
impl From<StackWindowError> for EffectError {
    fn from(error: StackWindowError) -> Self {
        Self::Window(error)
    }
}
impl From<TimingError> for EffectError {
    fn from(error: TimingError) -> Self {
        Self::Timing(error)
    }
}
impl From<CharacteristicsError> for EffectError {
    fn from(error: CharacteristicsError) -> Self {
        Self::Characteristics(error)
    }
}
impl From<CharacteristicInvariantError> for EffectError {
    fn from(error: CharacteristicInvariantError) -> Self {
        Self::CharacteristicInvariant(error)
    }
}
impl From<DamageError> for EffectError {
    fn from(error: DamageError) -> Self {
        Self::Damage(error)
    }
}
impl From<CheckError> for EffectError {
    fn from(error: CheckError) -> Self {
        Self::Check(error)
    }
}
impl From<CombatError> for EffectError {
    fn from(error: CombatError) -> Self {
        Self::Combat(error)
    }
}
impl From<crate::occurrence::TriggerError> for EffectError {
    fn from(error: crate::occurrence::TriggerError) -> Self {
        Self::Trigger(error)
    }
}
/// Immutable, canonical native state. This is not a safe network projection and
/// cannot be opened over arbitrary existing games: every object has a closed-world program.
/// Privileged snapshot of the existing owners' checked allocator positions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EffectCounters {
    pub next_event: u64,
    pub next_instance: u64,
    pub next_wave: u64,
    pub next_effect: u64,
    pub next_characteristic_timestamp: u64,
    pub next_world_tenure: u64,
    pub next_shield: u64,
    pub next_multiplier: u64,
    pub next_batch: u64,
    pub next_attempt: u64,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EffectWindow {
    revision: u64,
    window: StackWindow,
    programs: Arc<BTreeMap<CardId, Program>>,
    creature_vocabulary: Arc<CreatureTypeVocabulary>,
    name_rules: Arc<spellwake_contracts::names::NameRules>,
    payloads: BTreeMap<StackEntryId, StackPayload>,
    triggers: crate::occurrence::TriggerState,
    pending_work: Option<PendingWork>,
    damage: BTreeMap<ObjectId, RulesInteger>,
    mana: BTreeMap<(SeatId, ManaType), RulesInteger>,
    life: crate::life::LifeState,
    combat_rules: CombatRules,
    combat: Option<CombatState>,
    characteristics: CharacteristicState,
    damage_state: DamageState,
    failed_draws: BTreeSet<SeatId>,
    state_based_losses: BTreeSet<SeatId>,
}
/// The state, payment and current rules append are adopted together.
pub type CastTransition = RulesTransition<EffectWindow, ManaPaymentRecord>;
/// Fixed production transports the same append as its successful state.
pub type ManaActivationTransition = RulesTransition<EffectWindow, ManaProductionRecord>;
/// Immutable executor-issued token for an ability that has already paid its tap.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManaColorChoice {
    pub(crate) revision: u64,
    source: ObjectId,
    ability: crate::mana::ManaAbilityRef,
    chooser: SeatId,
    amount: RulesInteger,
    colors: BTreeSet<ManaType>,
}
impl ManaColorChoice {
    /// Exact executor revision at which this color choice was issued.
    pub fn revision(&self) -> u64 {
        self.revision
    }
    pub fn source(&self) -> ObjectId {
        self.source
    }
    pub fn ability(&self) -> crate::mana::ManaAbilityRef {
        self.ability
    }
    pub fn chooser(&self) -> SeatId {
        self.chooser
    }
    pub fn amount(&self) -> &RulesInteger {
        &self.amount
    }
    pub fn colors(&self) -> &BTreeSet<ManaType> {
        &self.colors
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ManaActivationResult {
    Complete { production: ManaProductionRecord },
    Pending { frame: ManaColorChoice },
}
pub type ManaActivationProgress = RulesTransition<EffectWindow, ManaActivationResult>;
/// Exactly one owned rules process, never parallel mutable continuation fields.
#[derive(Debug, Clone, PartialEq, Eq)]
enum PendingWork {
    LibraryTop(Box<LibraryTopWork>),
    ResolutionSelection(Box<ResolutionSelectionWork>),
    OptionalEffect(Box<OptionalEffectWork>),
    ResolutionPayment(Box<ResolutionPaymentWork>),
    TokenEntry(Box<TokenEntryWork>),
    Checks(CheckProcess),
    Combat(CombatCursor),
    Damage(DamageWork),
    Resolution(ResolutionCursor),
    ManaColor(Box<ManaColorWork>),
}

/// One payload owner for instruction bodies and ordinary original permanent
/// spells. Permanent copies/tokens are not represented by this tag.
#[derive(Debug, Clone, PartialEq, Eq)]
enum StackPayload {
    Instructions(Box<InstructionPayload>),
    Permanent,
}
#[derive(Debug, Clone, PartialEq, Eq)]
struct InstructionPayload {
    copiable_traits: Option<CopiableSpellTraits>,
    body: ExecutableBody,
    bindings: ExecutionBindings,
}
/// Instruction spells own their copy values separately from execution bindings.
/// Abilities have no spell traits; Some(empty) remains a nameless ordinary spell.
#[derive(Debug, Clone, PartialEq, Eq)]
struct CopiableSpellTraits {
    card_types: BTreeSet<CardType>,
    creature_subtypes: BTreeSet<CreatureTypeId>,
    names: spellwake_contracts::names::ObjectNames,
    supertypes: BTreeSet<crate::program::Supertype>,
    triggered: Vec<crate::program::TriggeredAbility>,
}
/// Borrowed projection of the existing original-or-owned trait source.
struct CopiableSpellTraitsRef<'a> {
    names: &'a spellwake_contracts::names::ObjectNames,
    supertypes: &'a BTreeSet<crate::program::Supertype>,
    card_types: &'a BTreeSet<CardType>,
    creature_subtypes: &'a BTreeSet<CreatureTypeId>,
    triggered: &'a [crate::program::TriggeredAbility],
}
impl CopiableSpellTraits {
    fn from_program(program: &Program) -> Self {
        Self {
            card_types: program.card_types.clone(),
            creature_subtypes: program.creature_subtypes.clone(),
            names: program.names.clone(),
            supertypes: program.supertypes.clone(),
            triggered: program.triggered.clone(),
        }
    }
}
impl StackPayload {
    fn from_body(body: ExecutableBody, copiable_traits: Option<CopiableSpellTraits>) -> Self {
        Self::Instructions(Box::new(InstructionPayload {
            copiable_traits,
            body,
            bindings: ExecutionBindings::default(),
        }))
    }
    fn body(&self) -> Option<&ExecutableBody> {
        match self {
            Self::Instructions(payload) => Some(&payload.body),
            Self::Permanent => None,
        }
    }
    fn instructions(&self) -> Option<&[Instruction]> {
        match self {
            Self::Instructions(payload) => Some(&payload.body.instructions),
            Self::Permanent => None,
        }
    }
}
/// Privileged borrowed payload inspection, independent of current stack membership.
/// This does not grant mutation, resumption or a safe public projection.
#[derive(Debug, Clone, Copy)]
pub struct RetainedStackPayload<'a> {
    /// Actual entry key under which the payload is retained.
    pub entry: StackEntryId,
    /// Entire body, including instructions not reached by the current cursor.
    pub body: Option<&'a ExecutableBody>,
    /// Complete owned copiable trigger definitions, independent of donor lifetime.
    pub copied_triggered: Option<&'a [crate::program::TriggeredAbility]>,
    /// Actual retained capture bindings, not reconstructed from current objects.
    pub bindings: Option<&'a ExecutionBindings>,
}
/// Points into the owned payload. Tokens never own another mutable program counter.
#[derive(Debug, Clone, PartialEq, Eq)]
struct ResolutionCursor {
    entry: StackEntryId,
    instruction: usize,
    values: ResolutionValues,
}
type ResolutionValues = BTreeMap<CreatureTypeSlot, CreatureTypeId>;

/// Damage temporarily owns the complete paused process, including its local values.
#[derive(Debug, Clone, PartialEq, Eq)]
struct DamageWork {
    batch: ActiveDamageBatch,
    parent: DamageParent,
}
#[derive(Debug, Clone, PartialEq, Eq)]
enum DamageParent {
    Resolution(ResolutionCursor),
    Combat,
}

/// Borrowed evidence of the sole live resolution cursor; this cannot mutate or resume it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResolutionState<'a> {
    entry: StackEntryId,
    instruction: usize,
    locals: &'a ResolutionValues,
}
impl<'a> ResolutionState<'a> {
    pub fn entry(&self) -> StackEntryId {
        self.entry
    }
    pub fn instruction(&self) -> usize {
        self.instruction
    }
    pub fn locals(&self) -> &'a BTreeMap<CreatureTypeSlot, CreatureTypeId> {
        self.locals
    }
}
impl<'a> From<&'a ResolutionCursor> for ResolutionState<'a> {
    fn from(cursor: &'a ResolutionCursor) -> Self {
        Self {
            entry: cursor.entry,
            instruction: cursor.instruction,
            locals: &cursor.values,
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DamageParentView<'a> {
    Resolution(ResolutionState<'a>),
    Combat,
}

/// Executor-issued choice domain; resolution values remain owned by the cursor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreatureTypeChoice {
    pub(crate) revision: u64,
    entry: StackEntryId,
    instruction: usize,
    slot: CreatureTypeSlot,
    chooser: SeatId,
    vocabulary: CreatureVocabularyId,
    excluding: BTreeSet<CreatureTypeId>,
}
impl CreatureTypeChoice {
    pub fn revision(&self) -> u64 {
        self.revision
    }
    pub fn entry(&self) -> StackEntryId {
        self.entry
    }
    pub fn instruction(&self) -> usize {
        self.instruction
    }
    pub fn slot(&self) -> CreatureTypeSlot {
        self.slot
    }
    pub fn chooser(&self) -> SeatId {
        self.chooser
    }
    pub fn vocabulary(&self) -> CreatureVocabularyId {
        self.vocabulary
    }
    pub fn excluding(&self) -> &BTreeSet<CreatureTypeId> {
        &self.excluding
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreatureView {
    pub object: ObjectId,
    pub power: RulesInteger,
    pub toughness: RulesInteger,
    pub damage: RulesInteger,
}

impl EffectWindow {
    pub fn new(seed: GameSeed) -> Result<Self, EffectError> {
        let GameSeed {
            seats: order,
            active,
            combat_rules,
            cards,
            initial_battlefield_order,
            initial_world_cohorts,
            mana,
            life,
            creature_vocabulary,
            name_rules,
        } = seed;
        let mut objects = GameObjects::new(0);
        let mut programs = BTreeMap::new();
        for seed in cards {
            seed.program
                .validate(&creature_vocabulary)
                .map_err(|_| EffectError::InvalidSetup)?;
            if matches!(&seed.program.kind, ProgramKind::Spell(_))
                && matches!(seed.destination, ObjectDestination::Battlefield { .. })
                || seed.program.has_type(CardType::Land)
                    && matches!(seed.destination, ObjectDestination::Stack { .. })
                || (seed.program.has_type(CardType::Artifact)
                    || seed.program.has_type(CardType::Enchantment))
                    && matches!(seed.destination, ObjectDestination::Stack { .. })
            {
                return Err(EffectError::InvalidSetup);
            }
            if programs.insert(seed.card, seed.program).is_some() {
                return Err(EffectError::InvalidSetup);
            }
            objects = objects
                .seed_card(objects.revision(), seed.card, seed.owner, seed.destination)
                .map_err(StackWindowError::from)?
                .next;
        }
        let mut pools = BTreeMap::new();
        for pool in mana {
            if !order.contains(&pool.player)
                || pool.amount < 0.into()
                || pools
                    .insert((pool.player, pool.mana_type), pool.amount)
                    .is_some()
            {
                return Err(EffectError::InvalidSetup);
            }
        }
        let mut characteristics = CharacteristicState::default();
        characteristics.register_initial_battlefield(&objects, &initial_battlefield_order)?;
        let evaluated = evaluate(CharacteristicWorld {
            cards: &objects,
            programs: &programs,
            state: &characteristics,
            vocabulary: &creature_vocabulary,
            name_rules: &name_rules,
        })?;
        let desired = desired_controllers(&objects, &evaluated)?;
        characteristics.initialize_world_tenures(&objects, &evaluated, &initial_world_cohorts)?;
        if desired
            .values()
            .any(|controller| !order.contains(controller))
        {
            return Err(EffectError::InvalidSetup);
        }
        if !desired.is_empty() {
            objects = objects
                .apply_effective_controllers(objects.revision(), &desired)
                .map_err(StackWindowError::from)?
                .0;
        }
        // Establish the trusted starting world before creating the one initial
        // window. No game transition, stack revision or event is invented here.
        let window = StackWindow::new(order, active, objects)?;
        let life = crate::life::LifeState::new(window.participants(), life)
            .ok_or(EffectError::InvalidSetup)?;
        Ok(Self {
            revision: 0,
            window,
            programs: Arc::new(programs),
            creature_vocabulary,
            name_rules,
            payloads: BTreeMap::new(),
            triggers: crate::occurrence::TriggerState::default(),
            pending_work: None,
            damage: BTreeMap::new(),
            mana: pools,
            life,
            combat_rules,
            combat: None,
            characteristics,
            damage_state: DamageState::default(),
            failed_draws: BTreeSet::new(),
            state_based_losses: BTreeSet::new(),
        })
    }
    pub fn window(&self) -> &StackWindow {
        &self.window
    }
    pub fn revision(&self) -> u64 {
        self.revision
    }
    /// Immutable authored definitions for physical cards in every zone.
    /// Use `copiable_program` for a selected definition including copy effects.
    pub fn programs(&self) -> impl Iterator<Item = (CardId, &Program)> {
        self.programs.iter().map(|(&card, program)| (card, program))
    }
    pub fn creature_vocabulary(&self) -> &CreatureTypeVocabulary {
        &self.creature_vocabulary
    }
    pub fn name_rules(&self) -> &spellwake_contracts::names::NameRules {
        &self.name_rules
    }
    pub fn resolution_state(&self) -> Option<ResolutionState<'_>> {
        match self.pending_work.as_ref() {
            Some(PendingWork::LibraryTop(work)) => Some((&work.cursor).into()),
            Some(PendingWork::ResolutionSelection(work)) => Some((&work.cursor).into()),
            Some(PendingWork::OptionalEffect(work)) => Some((&work.cursor).into()),
            Some(PendingWork::ResolutionPayment(work)) => Some((&work.cursor).into()),
            Some(PendingWork::ManaColor(work)) => match &work.parent {
                ManaParent::Priority => None,
                ManaParent::Payment(parent) => Some((&parent.cursor).into()),
            },
            Some(PendingWork::Resolution(cursor)) => Some(cursor.into()),
            Some(PendingWork::TokenEntry(work)) => Some((&work.cursor).into()),
            Some(PendingWork::Damage(DamageWork {
                parent: DamageParent::Resolution(cursor),
                ..
            })) => Some(cursor.into()),
            _ => None,
        }
    }
    pub fn damage_parent(&self) -> Option<DamageParentView<'_>> {
        match self.pending_work.as_ref() {
            Some(PendingWork::Damage(work)) => Some(match &work.parent {
                DamageParent::Resolution(cursor) => DamageParentView::Resolution(cursor.into()),
                DamageParent::Combat => DamageParentView::Combat,
            }),
            _ => None,
        }
    }
    pub fn continuous_effects(&self) -> impl Iterator<Item = &ResolvedContinuousEffect> {
        self.characteristics.effects()
    }
    /// Privileged canonical BF ordering facts; these are not entry-history events.
    pub fn characteristic_timestamps(
        &self,
    ) -> impl Iterator<Item = (ObjectId, CharacteristicTimestamp)> + '_ {
        self.characteristics.object_timestamps()
    }
    /// Canonical uninterrupted World cohorts for exact current battlefield objects.
    /// These are actual history, independent of copiable traits and ordering clocks.
    pub fn world_tenures(
        &self,
    ) -> impl Iterator<Item = (ObjectId, crate::characteristics::WorldTenure)> + '_ {
        self.characteristics.world_tenures()
    }
    /// Read-only operational allocators; these are not allocated semantic identities.
    pub fn operational_counters(&self) -> EffectCounters {
        let (next_event, next_instance, next_wave) = self.triggers.counters();
        let (next_shield, next_multiplier, next_batch, next_attempt) = self.damage_state.counters();
        EffectCounters {
            next_event,
            next_instance,
            next_wave,
            next_effect: self.characteristics.next_effect_sequence(),
            next_characteristic_timestamp: self.characteristics.next_timestamp_sequence(),
            next_world_tenure: self.characteristics.next_world_tenure_sequence(),
            next_shield,
            next_multiplier,
            next_batch,
            next_attempt,
        }
    }
    /// Privileged current captures, not a safe player-facing projection or history.
    pub fn waiting_triggers(&self) -> impl Iterator<Item = &WaitingInstance> {
        self.triggers.waiting().values()
    }
    /// Inspect every retained payload without relying on a live stack entry.
    /// No evaluation, normalization or cloning of owned source snapshots occurs.
    pub fn retained_stack_payloads(&self) -> impl Iterator<Item = RetainedStackPayload<'_>> {
        self.payloads.iter().map(|(&entry, payload)| match payload {
            StackPayload::Instructions(payload) => RetainedStackPayload {
                entry,
                body: Some(&payload.body),
                copied_triggered: payload
                    .copiable_traits
                    .as_ref()
                    .map(|traits| traits.triggered.as_slice()),
                bindings: Some(&payload.bindings),
            },
            StackPayload::Permanent => RetainedStackPayload {
                entry,
                body: None,
                copied_triggered: None,
                bindings: None,
            },
        })
    }
    pub fn stack_body(&self, entry: StackEntryId) -> Option<&ExecutableBody> {
        self.payloads.get(&entry).and_then(StackPayload::body)
    }
    /// Copiable names on a live spell entry. `Some(empty)` is a nameless spell;
    /// `None` is an ability or missing entry. Noncard copies own captured names,
    /// independent of the original entry's later lifetime. This is not a general
    /// current-characteristic query for noncard stack objects.
    pub fn stack_copiable_names(
        &self,
        entry: StackEntryId,
    ) -> Option<&spellwake_contracts::names::ObjectNames> {
        self.stack_copiable_traits(entry).map(|traits| traits.names)
    }
    /// Copiable supertypes of a live spell, including captured noncard copies.
    /// None denotes an ability or missing entry; Some(empty) is a spell without
    /// supertypes. This does not evaluate current layers on noncard objects.
    pub fn stack_copiable_supertypes(
        &self,
        entry: StackEntryId,
    ) -> Option<&BTreeSet<crate::program::Supertype>> {
        self.stack_copiable_traits(entry)
            .map(|traits| traits.supertypes)
    }
    /// Represented copiable card types of a live original or owned spell copy.
    /// Abilities and missing entries return None. Current noncard layers are absent.
    pub fn stack_copiable_card_types(&self, entry: StackEntryId) -> Option<&BTreeSet<CardType>> {
        self.stack_copiable_traits(entry)
            .map(|traits| traits.card_types)
    }
    /// Captured shared creature-family subtypes, independently of donor lifetime.
    pub fn stack_copiable_creature_subtypes(
        &self,
        entry: StackEntryId,
    ) -> Option<&BTreeSet<CreatureTypeId>> {
        self.stack_copiable_traits(entry)
            .map(|traits| traits.creature_subtypes)
    }
    /// Admitted copiable trigger definitions from the physical Program or owned copy.
    /// `None` denotes a nonspell/unavailable source; an empty slice is a spell
    /// without triggered definitions. This query does not enable stack discovery.
    pub fn stack_copiable_triggered_abilities(
        &self,
        entry: StackEntryId,
    ) -> Option<&[crate::program::TriggeredAbility]> {
        self.stack_copiable_traits(entry)
            .map(|traits| traits.triggered)
    }
    fn stack_copiable_traits(&self, entry: StackEntryId) -> Option<CopiableSpellTraitsRef<'_>> {
        let entry = self
            .window
            .entries()
            .iter()
            .find(|value| value.id() == entry)?;
        match entry.kind() {
            StackKind::CardSpell(source) => {
                self.copiable_program(*source)
                    .map(|program| CopiableSpellTraitsRef {
                        names: &program.names,
                        supertypes: &program.supertypes,
                        card_types: &program.card_types,
                        creature_subtypes: &program.creature_subtypes,
                        triggered: &program.triggered,
                    })
            }
            StackKind::SpellCopy { .. } => match self.payloads.get(&entry.id())? {
                StackPayload::Instructions(payload) => {
                    payload
                        .copiable_traits
                        .as_ref()
                        .map(|traits| CopiableSpellTraitsRef {
                            names: &traits.names,
                            supertypes: &traits.supertypes,
                            card_types: &traits.card_types,
                            creature_subtypes: &traits.creature_subtypes,
                            triggered: &traits.triggered,
                        })
                }
                StackPayload::Permanent => None,
            },
            StackKind::ActivatedAbility { .. } | StackKind::TriggeredAbility { .. } => None,
        }
    }
    pub fn execution_bindings(&self, entry: StackEntryId) -> Option<&ExecutionBindings> {
        match self.payloads.get(&entry)? {
            StackPayload::Instructions(payload) => Some(&payload.bindings),
            StackPayload::Permanent => None,
        }
    }
    /// Install only against the sole allocator's actual matching trigger entry.
    fn install_waiting_payload(
        &mut self,
        entry: StackEntryId,
        waiting: WaitingInstance,
    ) -> Result<(), EffectError> {
        let actual = self
            .window
            .entries()
            .iter()
            .find(|value| value.id() == entry)
            .ok_or(EffectError::InconsistentProgram)?;
        if actual.controller() != waiting.controller
            || !matches!(actual.kind(), StackKind::TriggeredAbility { source } if source == &waiting.source.object)
            || match waiting.body.target {
                BodyTarget::None => !actual.targets().targets().is_empty(),
                _ => actual.targets().targets().len() != 1,
            }
        {
            return Err(EffectError::InconsistentProgram);
        }
        let (body, trigger) = waiting.into_payload_parts();
        match self.payloads.entry(entry) {
            std::collections::btree_map::Entry::Vacant(slot) => {
                slot.insert(StackPayload::Instructions(Box::new(InstructionPayload {
                    copiable_traits: None,
                    body,
                    bindings: ExecutionBindings {
                        trigger: Some(trigger),
                    },
                })));
                Ok(())
            }
            std::collections::btree_map::Entry::Occupied(_) => {
                Err(EffectError::InconsistentProgram)
            }
        }
    }
    /// Privileged native total; absent players have no inferred balance.
    pub fn life_total(&self, player: SeatId) -> Option<&RulesInteger> {
        self.life.get(player)
    }
    /// Metadata-only bookkeeping; this operation emits no gameplay append.
    pub(crate) fn begin_opening(&self, expected: u64) -> Result<Self, EffectError> {
        let revision = self.next_revision(expected)?;
        if self.pending_work.is_some() || !self.state_based_losses.is_empty() {
            return Err(EffectError::WrongPhase);
        }
        Ok(Self {
            revision,
            window: self.window.begin_opening(self.window.revision())?,
            ..self.clone()
        })
    }
    fn next_revision(&self, expected: u64) -> Result<u64, EffectError> {
        if expected != self.revision {
            return Err(StackWindowError::StaleRevision.into());
        }
        self.revision
            .checked_add(1)
            .ok_or(StackWindowError::RevisionExhausted.into())
    }
    pub fn prevention_shields(&self) -> impl Iterator<Item = &PreventionShield> {
        self.damage_state.shields()
    }
    pub fn prevention_shield(&self, id: PreventionShieldId) -> Option<&PreventionShield> {
        self.damage_state.shield(id)
    }
    pub fn damage_multipliers(&self) -> impl Iterator<Item = &DamageMultiplier> {
        self.damage_state.multipliers()
    }
    pub fn damage_multiplier(&self, id: DamageMultiplierId) -> Option<&DamageMultiplier> {
        self.damage_state.multiplier(id)
    }
    pub fn pending_damage(&self) -> Option<&ActiveDamageBatch> {
        match &self.pending_work {
            Some(PendingWork::Damage(work)) => Some(&work.batch),
            _ => None,
        }
    }
    pub fn mana(&self, player: SeatId, mana_type: ManaType) -> RulesInteger {
        self.mana
            .get(&(player, mana_type))
            .cloned()
            .unwrap_or_else(|| 0.into())
    }
    /// The selected face-up copiable definition within the admitted partial
    /// Program language, before text changes and other later-layer effects.
    /// Includes represented printed names and supertypes. Missing physical
    /// incarnations return `None`; faces and other metadata outside Program
    /// remain unrepresented.
    pub fn copiable_program(&self, object: ObjectId) -> Option<&Program> {
        let physical = self.window.cards().get(object)?;
        self.characteristics
            .copiable_definition(physical, &self.programs)
            .map(|(program, _)| program)
    }
    /// The origin's baseline definition, independently of battlefield copy effects.
    pub fn base_program(&self, object: ObjectId) -> Option<&Program> {
        self.window
            .cards()
            .get(object)?
            .base_program(&self.programs)
    }
    /// Copiable printed PT, including captured copy exceptions and star formulas,
    /// independently of current numeric values.
    pub fn copiable_power_toughness(
        &self,
        object: ObjectId,
    ) -> Option<crate::characteristics::PrintedPowerToughness> {
        self.copiable_program(object)?
            .permanent()?
            .printed_pt
            .clone()
    }
    /// A fresh, pure result for this exact physical/definition/effect cut.
    /// Batch consumers compute it once and perform lookups in the returned map.
    pub fn evaluated_characteristics(&self) -> Result<EvaluatedCharacteristics, EffectError> {
        Ok(evaluate(CharacteristicWorld {
            cards: self.window.cards(),
            programs: &self.programs,
            state: &self.characteristics,
            vocabulary: &self.creature_vocabulary,
            name_rules: &self.name_rules,
        })?)
    }
    pub fn current_characteristics(
        &self,
        object: ObjectId,
    ) -> Result<Option<ObjectCharacteristics>, EffectError> {
        let mut evaluated = self.evaluated_characteristics()?;
        Ok(evaluated.objects.remove(&object))
    }
    /// A noncreature permanent has no current PT even when it has a literal pair.
    /// Current numeric PT in any represented zone (CR 208.3).
    ///
    /// # Errors
    /// Propagates an inconsistent supplied-world characteristic failure.
    pub fn power_toughness(
        &self,
        object: ObjectId,
    ) -> Result<Option<PowerToughnessEvaluation>, EffectError> {
        Ok(self
            .current_characteristics(object)?
            .and_then(|value| value.power_toughness))
    }
    /// Physical counters, independent of derived type or power/toughness.
    pub fn power_toughness_counters(&self, object: ObjectId) -> Option<PowerToughnessCounters> {
        self.window
            .cards()
            .get(object)
            .filter(|card| card.zone() == ObjectZone::Battlefield)?;
        Some(self.characteristics.counters(object))
    }
    /// Physical marks remain on a permanent through same-incarnation type loss.
    pub fn battlefield_marked_damage(&self, object: ObjectId) -> Option<RulesInteger> {
        self.window
            .cards()
            .get(object)
            .filter(|card| card.zone() == ObjectZone::Battlefield)?;
        Some(
            self.damage
                .get(&object)
                .cloned()
                .unwrap_or_else(|| 0.into()),
        )
    }
    pub fn continuous_effect(&self, id: ContinuousEffectId) -> Option<&ResolvedContinuousEffect> {
        self.characteristics.effect(id)
    }
    /// Current battlefield creature and its physical marked damage.
    ///
    /// # Errors
    /// Propagates an inconsistent supplied-world characteristic failure.
    pub fn creature(&self, object: ObjectId) -> Result<Option<CreatureView>, EffectError> {
        Ok(self
            .current_characteristics(object)?
            .filter(|current| current.zone == ObjectZone::Battlefield && current.is_creature())
            .and_then(|current| current.power_toughness)
            .map(|evaluation| CreatureView {
                object,
                power: evaluation.derived.power,
                toughness: evaluation.derived.toughness,
                damage: self
                    .damage
                    .get(&object)
                    .cloned()
                    .unwrap_or_else(|| 0.into()),
            }))
    }
    /// Used direct-fixture adapter over the shared resumable checks engine.
    /// A genuine owner arrangement requires begin_checks/resume_rule; this
    /// adapter never chooses a graveyard order on a player's behalf.
    /// Receipt-dropping convenience for direct fixtures; root consumes begin_checks instead.
    pub fn settle_checks(&self, expected: u64) -> Result<Self, EffectError> {
        match self.begin_checks(expected)?.into_parts() {
            (state, RuleResult::Complete { .. }, _) => Ok(state),
            (_, RuleResult::Pending { .. }, _) => Err(CheckError::ChoiceRequired.into()),
        }
    }
    /// Adjudicated life and empty-draw losses. Multiplayer departure cleanup is not yet
    /// implemented; callers must stop before granting priority on this boundary.
    pub fn state_based_losses(&self) -> &BTreeSet<SeatId> {
        &self.state_based_losses
    }
    pub(crate) fn draw_cards(
        &self,
        expected: u64,
        player: SeatId,
        amount: RulesInteger,
    ) -> Result<RulesTransition<Self, DrawJournal>, EffectError> {
        let origin = if self.window.phase() == WindowPhase::Opening {
            DrawOrigin::Opening
        } else {
            DrawOrigin::MechanismUnit
        };
        self.draw_cards_with_origin(expected, player, amount, origin)
    }
    pub(crate) fn draw_cards_with_origin(
        &self,
        expected: u64,
        player: SeatId,
        amount: RulesInteger,
        origin: DrawOrigin,
    ) -> Result<RulesTransition<Self, DrawJournal>, EffectError> {
        let revision = self.next_revision(expected)?;
        if self.pending_work.is_some() {
            return Err(EffectError::WrongPhase);
        }
        if !self.state_based_losses.is_empty() {
            return Err(EffectError::PlayerLossPending);
        }
        let mut next = Self {
            revision,
            ..self.clone()
        };
        let mut append = RulesJournal::default();
        let journal = next.draw_at_cuts(player, amount, origin, &mut append)?;
        Ok(RulesTransition::new(next, journal, append))
    }
    /// Native turn lifecycle, never an ordinary player action. Pending work and
    /// open stack/priority rounds cannot be replaced by an arbitrary next step.
    pub(crate) fn enter_turn_step(
        &self,
        expected: u64,
        active: SeatId,
        priority: bool,
        empty_mana: bool,
    ) -> Result<RulesTransition<Self, ()>, EffectError> {
        let revision = self.next_revision(expected)?;
        if self.pending_work.is_some() {
            return Err(EffectError::WrongPhase);
        }
        if !self.state_based_losses.is_empty() {
            return Err(EffectError::PlayerLossPending);
        }
        let window = self
            .window
            .enter_turn_step(self.window.revision(), active, priority)?;
        let mut next = Self {
            revision,
            window,
            ..self.clone()
        };
        if empty_mana {
            next.mana.clear();
        }
        Ok(RulesTransition::new(next, (), RulesJournal::default()))
    }
    /// Actual controller-turn start, separately from untapping. The root calls
    /// this only at an established turn start, including the opening handoff.
    /// Metadata-only bookkeeping; this operation emits no gameplay append.
    pub(crate) fn record_turn_start(&self, expected: u64) -> Result<Self, EffectError> {
        let revision = self.next_revision(expected)?;
        if self.pending_work.is_some() {
            return Err(EffectError::WrongPhase);
        }
        if !self.state_based_losses.is_empty() {
            return Err(EffectError::PlayerLossPending);
        }
        Ok(Self {
            revision,
            window: self.window.record_turn_start(self.window.revision())?,
            ..self.clone()
        })
    }
    /// Metadata-only bookkeeping; this operation emits no gameplay append.
    pub fn pass(&self, expected: u64, player: SeatId) -> Result<Self, EffectError> {
        let revision = self.next_revision(expected)?;
        self.ensure_resolution_or_idle()?;
        Ok(Self {
            revision,
            window: self.window.pass(self.window.revision(), player)?,
            ..self.clone()
        })
    }
    fn validate_current_source(actor: SeatId, card: &GameObject) -> Result<(), EffectError> {
        if card.zone() != ObjectZone::Battlefield {
            return Err(StackWindowError::from(
                crate::object_identity::ObjectIdentityError::NotPermanent,
            )
            .into());
        }
        if card.controller() != Some(actor) {
            return Err(StackWindowError::WrongController.into());
        }
        Ok(())
    }
    fn validate_creature_tap_history(
        card: &GameObject,
        current: &ObjectCharacteristics,
    ) -> Result<(), EffectError> {
        if current.is_creature()
            && card.has_continuous_control_since_turn_start() != Some(true)
            && !current.has_keyword(KeywordAbility::Haste)
        {
            return Err(EffectError::CreatureTapRestricted);
        }
        Ok(())
    }
    fn prepare_mana_payment(
        &self,
        cost: &ManaCost,
        selected: Option<&ManaPayment>,
        actor: SeatId,
    ) -> Result<Vec<ManaSpend>, EffectError> {
        prepare_payment(cost, selected, |mana_type| self.mana(actor, mana_type)).map_err(|error| {
            match error {
                ManaPaymentError::InsufficientMana => EffectError::InsufficientMana,
                ManaPaymentError::PaymentRequired => EffectError::ManaPaymentRequired,
                other => EffectError::InvalidManaPayment(other),
            }
        })
    }
    fn debit_mana(&mut self, actor: SeatId, spend: &[ManaSpend]) {
        for payment in spend {
            self.mana.insert(
                (actor, payment.mana_type),
                self.mana(actor, payment.mana_type).minus(&payment.amount),
            );
        }
    }
    /// Complete this closed activation atomically. Mana already floats; no nested
    /// payment abilities or general activation/reversal protocol here.
    pub fn activate_ability(
        &self,
        expected: u64,
        actor: SeatId,
        source: ObjectId,
        ability: ActivatedAbilityId,
        target: TargetRef,
        payment: Option<ManaPayment>,
    ) -> Result<AbilityActivationTransition, EffectError> {
        let revision = self.next_revision(expected)?;
        self.ensure_resolution_or_idle()?;
        let card = self
            .window
            .cards()
            .get(source)
            .ok_or(EffectError::UnknownObject)?;
        let evaluated = self.evaluated_characteristics()?;
        let current = evaluated
            .get(source)
            .ok_or(EffectError::InconsistentProgram)?;
        if current.abilities.activated.is_empty() {
            return Err(EffectError::NotAbilitySource);
        }
        if !current.abilities.activated.contains(&ability) {
            return Err(EffectError::UnknownActivatedAbility);
        }
        self.window.validate_action_actor(actor)?;
        Self::validate_current_source(actor, card)?;
        let selected = current
            .printed_text
            .activated
            .iter()
            .find(|entry| entry.id == ability)
            .ok_or(EffectError::UnknownActivatedAbility)?;
        if selected.source_cost == crate::activated::SourceCost::Tap {
            Self::validate_creature_tap_history(card, current)?;
        }
        if !self.legal_body_target(&selected.body, target)? {
            return Err(EffectError::IllegalTarget);
        }
        let spend = self.prepare_mana_payment(&selected.mana_cost, payment.as_ref(), actor)?;
        let announced = self.window.record_targeted_activation(
            self.window.revision(),
            actor,
            card.clone(),
            vec![TargetClause {
                id: TargetClauseId(0),
                minimum: 1,
                maximum: 1,
            }],
            vec![TargetChoice {
                id: TargetId(0),
                clause: TargetClauseId(0),
                target,
            }],
        )?;
        let entry = announced
            .entries()
            .last()
            .ok_or(EffectError::InconsistentProgram)?
            .id();
        let mut next = self.clone();
        next.revision = revision;
        next.window = announced;
        next.payloads
            .insert(entry, StackPayload::from_body(selected.body.clone(), None));
        // The actual entry and captured body exist before any source-cost write.
        let cost_before = next.rules_snapshot(None)?;
        let (window, source_after) =
            next.window
                .pay_announced_source_cost(entry, actor, source, selected.source_cost)?;
        next.window = window;
        let activation = AbilityActivationRecord {
            entry,
            ability,
            controller: actor,
            source: card.clone(),
            source_cost: selected.source_cost,
            resulting_source: source_after.id(),
        };
        let receipt = ManaPaymentRecord {
            source: entry,
            payer: actor,
            cost: selected.mana_cost.clone(),
            spend,
        };
        let mut append = RulesJournal::default();
        next.emit_from(
            crate::occurrence::EventCause::ActivatedAbilityCost { entry, ability },
            &cost_before,
            None,
            &mut append,
            |previous, current, controllers| {
                Ok(vec![match selected.source_cost {
                    crate::activated::SourceCost::None => return Ok(vec![]),
                    crate::activated::SourceCost::Tap => {
                        crate::occurrence::RulesOccurrence::StatusChange {
                            change: crate::object_identity::ObjectStatusChange {
                                before: activation.source.clone(),
                                after: source_after.clone(),
                            },
                            reason: crate::occurrence::StatusReason::ActivatedAbilityCost,
                        }
                    }
                    crate::activated::SourceCost::Sacrifice => zone_occurrence_at(
                        previous,
                        current,
                        &crate::object_identity::ObjectMovement {
                            before: activation.source.clone(),
                            after: source_after.clone(),
                        },
                        crate::occurrence::ZoneReason::SacrificeCost,
                        controllers,
                    )?,
                }])
            },
        )?;
        next.debit_mana_at_cut(&receipt, None, &mut append)?;
        let complete = next.rules_snapshot(None)?;
        next.emit_from(
            crate::occurrence::EventCause::ActivatedAbilityCompleted { entry, ability },
            &complete,
            None,
            &mut append,
            |_, _, _| {
                Ok(vec![crate::occurrence::RulesOccurrence::ActivatedAbility {
                    entry,
                    source: activation.source.clone(),
                    ability,
                }])
            },
        )?;
        Ok(AbilityActivationTransition::new(
            next,
            crate::activated::AbilityActivationResult {
                activation,
                payment: receipt,
            },
            append,
        ))
    }
    /// Synchronous fixed/count/intrinsic-output convenience. A color choice
    /// returns no accepted state.
    ///
    /// # Errors
    /// Returns the same atomic rejections as `begin_mana_activation`, or
    /// `ManaColorChoiceRequired` if the selected ability needs a color choice.
    pub fn activate_mana(
        &self,
        expected: u64,
        actor: SeatId,
        source: ObjectId,
        ability: crate::mana::ManaAbilityRef,
    ) -> Result<ManaActivationTransition, EffectError> {
        let (state, result, append) = self
            .begin_mana_activation(expected, actor, source, ability)?
            .into_parts();
        match result {
            ManaActivationResult::Complete { production } => {
                Ok(ManaActivationTransition::new(state, production, append))
            }
            ManaActivationResult::Pending { .. } => Err(EffectError::ManaColorChoiceRequired),
        }
    }
    /// Pay the tap cost, then resolve immediately or suspend for an effect-time color.
    /// No stack object, priority, checks or pool credit occurs during the choice.
    ///
    /// # Errors
    /// Rejects stale revisions, incompatible pending work, absent source/current
    /// reference, wrong actor/controller/zone, tapped source or Creature tap-history
    /// restrictions. Any rejection preserves the entire borrowed state.
    pub fn begin_mana_activation(
        &self,
        expected: u64,
        actor: SeatId,
        source: ObjectId,
        ability: crate::mana::ManaAbilityRef,
    ) -> Result<ManaActivationProgress, EffectError> {
        let revision = self.next_revision(expected)?;
        if !matches!(self.pending_work, Some(PendingWork::ResolutionPayment(_))) {
            self.ensure_resolution_or_idle()?;
        }
        let card = self
            .window
            .cards()
            .get(source)
            .ok_or(EffectError::UnknownObject)?;
        let evaluated = self.evaluated_characteristics()?;
        let current = evaluated
            .get(source)
            .ok_or(EffectError::InconsistentProgram)?;
        if current.abilities.mana.is_empty() && current.intrinsic_mana.is_empty() {
            return Err(EffectError::NotManaSource);
        }
        // Availability and semantic output come from the same evaluated cut.
        // Intrinsic output is temporary query data, never an authored definition.
        let selected = match ability {
            crate::mana::ManaAbilityRef::Printed(id) => {
                if !current.abilities.mana.contains(&id) {
                    return Err(EffectError::UnknownManaAbility);
                }
                std::borrow::Cow::Borrowed(
                    &current
                        .printed_text
                        .mana
                        .iter()
                        .find(|entry| entry.id == id)
                        .ok_or(EffectError::UnknownManaAbility)?
                        .output,
                )
            }
            crate::mana::ManaAbilityRef::BasicLand(kind) => {
                if !current.intrinsic_mana.contains(&kind) {
                    return Err(EffectError::UnknownManaAbility);
                }
                std::borrow::Cow::Owned(ManaOutput::Fixed(BTreeMap::from([(
                    kind.mana_type(),
                    1.into(),
                )])))
            }
        };
        let parent = self.mana_activation_parent(actor)?;
        Self::validate_current_source(actor, card)?;
        Self::validate_creature_tap_history(card, current)?;
        let window = match &parent {
            ManaParent::Priority => {
                self.window
                    .record_tap_action(self.window.revision(), actor, source)?
            }
            ManaParent::Payment(work) => self.window.record_resolution_tap(
                self.window.revision(),
                work.cursor.entry,
                actor,
                source,
            )?,
        };
        let mut next = self.clone();
        next.revision = revision;
        next.window = window;
        let mut append = RulesJournal::default();
        let before = self.rules_snapshot(None)?;
        let after = next
            .window
            .cards()
            .get(source)
            .ok_or(EffectError::InconsistentProgram)?
            .clone();
        next.emit_from(
            crate::occurrence::EventCause::ManaAbilityCost { source, ability },
            &before,
            None,
            &mut append,
            |_, _, _| {
                Ok(occurrence_flow::status_occurrences(
                    vec![crate::object_identity::ObjectStatusChange {
                        before: card.clone(),
                        after: after.clone(),
                    }],
                    crate::occurrence::StatusReason::ManaAbilityCost,
                ))
            },
        )?;
        let activated = next.rules_snapshot(None)?;
        next.emit_from(
            crate::occurrence::EventCause::ManaAbilityActivated { source, ability },
            &activated,
            None,
            &mut append,
            |_, _, _| {
                Ok(vec![crate::occurrence::RulesOccurrence::ManaAbility {
                    source: after,
                    ability,
                    controller: actor,
                }])
            },
        )?;
        match selected.as_ref() {
            ManaOutput::Counted {
                mana_type,
                permanents,
            } => {
                let evaluated = next.evaluated_characteristics()?;
                let amount = crate::characteristics::count_permanents(
                    next.window.cards(),
                    &evaluated,
                    source,
                    actor,
                    permanents,
                    &next.name_rules,
                );
                let produced = if amount.is_positive() {
                    BTreeMap::from([(*mana_type, amount)])
                } else {
                    BTreeMap::new()
                };
                let production =
                    next.credit_mana_at_cut(source, ability, actor, produced, &mut append)?;
                Ok(ManaActivationProgress::new(
                    next,
                    ManaActivationResult::Complete { production },
                    append,
                ))
            }
            ManaOutput::Fixed(produced) => {
                let production =
                    next.credit_mana_at_cut(source, ability, actor, produced.clone(), &mut append)?;
                Ok(ManaActivationProgress::new(
                    next,
                    ManaActivationResult::Complete { production },
                    append,
                ))
            }
            ManaOutput::ChooseColor { amount, colors } => {
                let frame = ManaColorChoice {
                    revision,
                    source,
                    ability,
                    chooser: actor,
                    amount: amount.clone(),
                    colors: colors.clone(),
                };
                next.pending_work = Some(PendingWork::ManaColor(Box::new(ManaColorWork {
                    frame: frame.clone(),
                    parent,
                })));
                Ok(ManaActivationProgress::new(
                    next,
                    ManaActivationResult::Pending { frame },
                    append,
                ))
            }
        }
    }
    pub fn pending_mana_color(&self) -> Option<&ManaColorChoice> {
        match self.pending_work.as_ref() {
            Some(PendingWork::ManaColor(work)) => Some(&work.frame),
            _ => None,
        }
    }
    fn ensure_resolution_or_idle(&self) -> Result<(), EffectError> {
        match self.pending_work.as_ref() {
            None | Some(PendingWork::Resolution(_)) => Ok(()),
            Some(
                PendingWork::LibraryTop(_)
                | PendingWork::ResolutionSelection(_)
                | PendingWork::TokenEntry(_)
                | PendingWork::OptionalEffect(_)
                | PendingWork::ResolutionPayment(_)
                | PendingWork::ManaColor(_)
                | PendingWork::Combat(_)
                | PendingWork::Damage(_)
                | PendingWork::Checks(_),
            ) => Err(EffectError::WrongPhase),
        }
    }
    fn resolution_cursor(&self) -> Option<&ResolutionCursor> {
        match self.pending_work.as_ref() {
            Some(PendingWork::LibraryTop(work)) => Some(&work.cursor),
            Some(PendingWork::ResolutionSelection(work)) => Some(&work.cursor),
            Some(PendingWork::Resolution(cursor)) => Some(cursor),
            Some(PendingWork::TokenEntry(work)) => Some(&work.cursor),
            _ => None,
        }
    }
    fn resolution_cursor_mut(&mut self) -> Option<&mut ResolutionCursor> {
        match self.pending_work.as_mut() {
            Some(PendingWork::LibraryTop(work)) => Some(&mut work.cursor),
            Some(PendingWork::ResolutionSelection(work)) => Some(&mut work.cursor),
            Some(PendingWork::Resolution(cursor)) => Some(cursor),
            Some(PendingWork::TokenEntry(work)) => Some(&mut work.cursor),
            _ => None,
        }
    }
    fn credit_mana(
        &mut self,
        source: ObjectId,
        ability: crate::mana::ManaAbilityRef,
        controller: SeatId,
        produced: BTreeMap<ManaType, RulesInteger>,
    ) -> ManaProductionRecord {
        for (mana_type, amount) in &produced {
            self.mana.insert(
                (controller, *mana_type),
                self.mana(controller, *mana_type).plus(amount),
            );
        }
        ManaProductionRecord {
            source,
            ability,
            controller,
            produced,
        }
    }
    /// Resume only this exact paid-cost choice. Invalid input preserves the tap.
    pub fn resume_mana_color(
        &self,
        expected: u64,
        frame: &ManaColorChoice,
        actor: SeatId,
        color: ManaType,
    ) -> Result<ManaActivationTransition, EffectError> {
        let revision = self.next_revision(expected)?;
        if self.pending_mana_color() != Some(frame) || frame.revision != self.revision {
            return Err(EffectError::WrongManaColorFrame);
        }
        if frame.chooser != actor {
            return Err(EffectError::WrongChooser);
        }
        if !frame.colors.contains(&color) {
            return Err(EffectError::IllegalManaColor);
        }
        let mut next = self.clone();
        next.revision = revision;
        let Some(PendingWork::ManaColor(work)) = next.pending_work.take() else {
            return Err(EffectError::WrongManaColorFrame);
        };
        next.pending_work = match work.parent {
            ManaParent::Priority => None,
            ManaParent::Payment(parent) => Some(PendingWork::ResolutionPayment(Box::new(parent))),
        };
        let mut append = RulesJournal::default();
        let production = next.credit_mana_at_cut(
            frame.source,
            frame.ability,
            actor,
            BTreeMap::from([(color, frame.amount.clone())]),
            &mut append,
        )?;
        Ok(ManaActivationTransition::new(next, production, append))
    }
    /// Cast with the unique mandatory payment for a cost containing no generic mana.
    /// Explicit generic allocation uses the same cast_with_payment path.
    /// Receipt-dropping direct-fixture convenience; root consumes cast_on_turn instead.
    pub fn cast(
        &self,
        expected: u64,
        actor: SeatId,
        spell: ObjectId,
        target: TargetRef,
    ) -> Result<Self, EffectError> {
        Ok(self
            .cast_with_payment(expected, actor, spell, target, None)?
            .into_state())
    }
    /// Atomically validate the authored cost and exact selected spend, then admit
    /// the supported spell. No general announcement or mana-ability flow is implied.
    pub fn cast_with_payment(
        &self,
        expected: u64,
        actor: SeatId,
        spell: ObjectId,
        target: TargetRef,
        selected: Option<ManaPayment>,
    ) -> Result<CastTransition, EffectError> {
        self.cast_on_turn(expected, actor, spell, Some(target), selected, None)
    }
    /// Validate CR205.4e against one exact current world before constructing a
    /// payload, preparing payment, or announcing a physical spell. The current
    /// admitted type family has creatures but no planeswalkers; adding that real
    /// type must extend the qualifying-permanent predicate here.
    fn validate_legendary_spell_cast(
        &self,
        spell: ObjectId,
        actor: SeatId,
        current: &crate::characteristics::EvaluatedCharacteristics,
    ) -> Result<(), EffectError> {
        use crate::program::Supertype;
        let candidate = current.get(spell).ok_or(EffectError::InconsistentProgram)?;
        let restricted = candidate.current_supertypes.contains(&Supertype::Legendary)
            && (candidate.current_types.contains(&CardType::Instant)
                || candidate.current_types.contains(&CardType::Sorcery));
        if restricted
            && !current.objects.values().any(|object| {
                object.zone == ObjectZone::Battlefield
                    && object.controller == Some(actor)
                    && object.current_supertypes.contains(&Supertype::Legendary)
                    && object.is_creature()
            })
        {
            return Err(EffectError::LegendarySpellPrerequisite);
        }
        Ok(())
    }
    /// Root-owned schedule is borrowed, not accepted from a player command.
    /// Calendar-free direct fixtures support instants only.
    pub(crate) fn cast_on_turn(
        &self,
        expected: u64,
        actor: SeatId,
        spell: ObjectId,
        target: Option<TargetRef>,
        selected: Option<ManaPayment>,
        turn: Option<&TurnScheduler>,
    ) -> Result<CastTransition, EffectError> {
        let revision = self.next_revision(expected)?;
        self.ensure_resolution_or_idle()?;
        let source = self
            .window
            .cards()
            .get(spell)
            .ok_or(EffectError::UnknownObject)?;
        if source.zone() != ObjectZone::Hand(actor) {
            return Err(EffectError::NotOwnHand);
        }
        let program = source
            .card_id()
            .and_then(|card| self.programs.get(&card))
            .ok_or(EffectError::NotCastableSpell)?;
        let current = self.evaluated_characteristics()?;
        self.validate_legendary_spell_cast(spell, actor, &current)?;
        let current_spell = current.get(spell).ok_or(EffectError::InconsistentProgram)?;
        let (cost, payload) = match &program.kind {
            ProgramKind::Spell(spell) => {
                if !current_spell.has_type(CardType::Instant) {
                    validate_main_phase(&self.window, turn, actor)?;
                }
                (
                    &spell.cost,
                    StackPayload::from_body(
                        spell.body.clone(),
                        Some(CopiableSpellTraits::from_program(program)),
                    ),
                )
            }
            ProgramKind::Permanent(permanent) if !program.has_type(CardType::Land) => {
                let cost = permanent
                    .casting_cost
                    .as_ref()
                    .ok_or(EffectError::NotCastableSpell)?;
                validate_main_phase(&self.window, turn, actor)?;
                (cost, StackPayload::Permanent)
            }
            _ => return Err(EffectError::NotCastableSpell),
        };
        let target_contract = payload
            .body()
            .map(|body| body.target)
            .unwrap_or(BodyTarget::None);
        match (target_contract, target) {
            (BodyTarget::None, Some(_)) => return Err(EffectError::UnexpectedTarget),
            (BodyTarget::None, None) => {}
            (_, None) => return Err(EffectError::TargetRequired),
            (_, Some(target)) => {
                if !self.legal_body_target(
                    payload.body().ok_or(EffectError::InconsistentProgram)?,
                    target,
                )? {
                    return Err(EffectError::IllegalTarget);
                }
            }
        }
        let spend = self.prepare_mana_payment(cost, selected.as_ref(), actor)?;
        let window = if let Some(target) = target {
            self.window.record_targeted_card_spell(
                self.window.revision(),
                actor,
                spell,
                vec![TargetClause {
                    id: TargetClauseId(0),
                    minimum: 1,
                    maximum: 1,
                }],
                vec![TargetChoice {
                    id: TargetId(0),
                    clause: TargetClauseId(0),
                    target,
                }],
            )?
        } else {
            self.window
                .record_card_spell(self.window.revision(), actor, spell)?
        };
        let entry = window
            .entries()
            .last()
            .ok_or(EffectError::InconsistentProgram)?
            .id();
        let mut next = self.clone();
        next.revision = revision;
        next.payloads.insert(entry, payload);
        next.window = window;
        let mut append = RulesJournal::default();
        let announcement = self.rules_snapshot(turn)?;
        let StackKind::CardSpell(admitted_object) = next
            .window
            .entries()
            .last()
            .ok_or(EffectError::InconsistentProgram)?
            .kind()
        else {
            return Err(EffectError::InconsistentProgram);
        };
        let movement = crate::object_identity::ObjectMovement {
            before: source.clone(),
            after: next
                .window
                .cards()
                .get(*admitted_object)
                .ok_or(EffectError::InconsistentProgram)?
                .clone(),
        };
        next.emit_from(
            crate::occurrence::EventCause::CastAnnouncement { entry },
            &announcement,
            turn,
            &mut append,
            |previous, current, controllers| {
                Ok(vec![zone_occurrence_at(
                    previous,
                    current,
                    &movement,
                    crate::occurrence::ZoneReason::CastAnnouncement,
                    controllers,
                )?])
            },
        )?;
        let receipt = ManaPaymentRecord {
            source: entry,
            payer: actor,
            cost: cost.clone(),
            spend,
        };
        next.debit_mana_at_cut(&receipt, turn, &mut append)?;
        let completed = next.rules_snapshot(turn)?;
        next.emit_from(
            crate::occurrence::EventCause::CastCompleted { entry },
            &completed,
            turn,
            &mut append,
            |_, current, _| {
                Ok(vec![crate::occurrence::RulesOccurrence::CastCompleted(
                    Box::new(current.cast_occurrence(entry, actor)?),
                )])
            },
        )?;
        Ok(CastTransition::new(next, receipt, append))
    }
    /// Ordinary authored land admission; the root owns the per-turn allowance.
    pub(crate) fn play_land(
        &self,
        expected: u64,
        actor: SeatId,
        land: ObjectId,
        turn: &TurnScheduler,
    ) -> Result<RulesTransition<Self, LandPlayRecord>, EffectError> {
        let revision = self.next_revision(expected)?;
        self.ensure_resolution_or_idle()?;
        validate_main_phase(&self.window, Some(turn), actor)?;
        let source = self
            .window
            .cards()
            .get(land)
            .ok_or(EffectError::UnknownObject)?;
        if source.zone() != ObjectZone::Hand(actor) {
            return Err(EffectError::NotOwnHand);
        }
        if !matches!(
            source.card_id().and_then(|card| self.programs.get(&card)),
            Some(program) if program.has_type(CardType::Land)
        ) {
            return Err(EffectError::NotLand);
        }
        let (window, mut record) =
            self.window
                .record_land_play(self.window.revision(), actor, land)?;
        let before = self.rules_snapshot(Some(turn))?;
        let mut next = Self {
            revision,
            window,
            ..self.clone()
        };
        next.characteristics
            .register_battlefield_entry(record.after.id())?;
        let movement = crate::object_identity::ObjectMovement {
            before: record.before.clone(),
            after: record.after.clone(),
        };
        let mut append = RulesJournal::default();
        next.emit_from(
            crate::occurrence::EventCause::LandPlay { actor },
            &before,
            Some(turn),
            &mut append,
            |previous, current, controllers| {
                Ok(vec![zone_occurrence_at(
                    previous,
                    current,
                    &movement,
                    crate::occurrence::ZoneReason::LandPlay,
                    controllers,
                )?])
            },
        )?;
        record.after = next
            .window
            .cards()
            .get(record.after.id())
            .ok_or(EffectError::InconsistentProgram)?
            .clone();
        Ok(RulesTransition::new(next, record, append))
    }
    /// Cross-layer overflow fault injection; absent from production builds.
    #[cfg(test)]
    pub(crate) fn set_test_revision(&mut self, revision: u64) {
        self.revision = revision;
    }
    #[cfg(test)]
    pub(crate) fn set_test_characteristic_timestamp(&mut self, next: u64) {
        self.characteristics.qualification_set_next_timestamp(next);
    }
    #[cfg(test)]
    pub(crate) fn set_test_publication_event(&mut self, next: u64) {
        self.triggers.set_test_next_event(next);
    }
    #[cfg(test)]
    pub(crate) fn set_test_publication_instance(&mut self, next: u64) {
        self.triggers.set_test_next_instance(next);
    }
    #[cfg(test)]
    pub(crate) fn set_test_window_revision(&mut self, revision: u64) {
        self.window.set_test_revision(revision);
    }
    #[cfg(test)]
    pub(crate) fn set_test_stack_next_entry(&mut self, next_entry: u64) {
        self.window.set_test_next_entry(next_entry);
    }
    #[cfg(test)]
    pub(crate) fn set_test_object_allocator(&mut self, value: u64, identity: bool) {
        if identity {
            self.window.set_test_next_object_identity(value);
        } else {
            self.window.set_test_object_revision(value);
        }
    }
    #[cfg(test)]
    pub(crate) fn set_test_mana_revision(&mut self, revision: u64) -> ManaColorChoice {
        self.revision = revision;
        let Some(PendingWork::ManaColor(work)) = &mut self.pending_work else {
            panic!("mana fault injection needs a color choice")
        };
        work.frame.revision = revision;
        work.frame.clone()
    }
    #[cfg(test)]
    pub(crate) fn set_test_priority_revision(&mut self, revision: u64) {
        self.window.set_test_priority_revision(revision);
    }
    /// Execute the selected supported stack payload. Identity and creature-type checks
    /// are computed natively, not supplied as caller booleans. Leaves CR 117.5
    /// pending so lethal creatures remain until settle_checks, not mid-resolution.
    /// Receipt-dropping direct-fixture convenience; never used by root adoption.
    pub fn resolve_top(&self, expected: u64) -> Result<Self, EffectError> {
        self.resolve_with_choice(expected, None)
    }
    /// Explicit resolution-time target decision when copying a targeted body.
    /// Receipt-dropping direct-fixture convenience; root uses the preserving continuation.
    ///
    /// # Errors
    /// Returns the underlying resolution/frame/choice error. An unused choice
    /// after automatic completion returns `UnexpectedCopyChoice`; another actual
    /// pending frame returns its required-choice error.
    pub fn resolve_top_with_copy_choice(
        &self,
        expected: u64,
        actor: SeatId,
        choice: CopyTargetChoice,
    ) -> Result<Self, EffectError> {
        self.resolve_with_choice(expected, Some((actor, choice)))
    }
    fn resolve_with_choice(
        &self,
        expected: u64,
        copy_choice: Option<(SeatId, CopyTargetChoice)>,
    ) -> Result<Self, EffectError> {
        let progress = self.begin_resolution(expected)?;
        match progress.into_parts() {
            (state, RuleResult::Complete { .. }, _) => {
                if copy_choice.is_some() {
                    return Err(EffectError::UnexpectedCopyChoice);
                }
                Ok(state)
            }
            (
                state,
                RuleResult::Pending {
                    frame: EffectDecision::CopyTargets(frame),
                    ..
                },
                _,
            ) => {
                let (actor, choice) = copy_choice.ok_or(EffectError::CopyChoiceRequired)?;
                match state
                    .resume_rule(
                        state.revision(),
                        &EffectDecision::CopyTargets(frame),
                        actor,
                        DecisionReply::CopyTargets(choice),
                        None,
                    )?
                    .into_parts()
                {
                    (state, RuleResult::Complete { .. }, _) => Ok(state),
                    (_, RuleResult::Pending { frame, .. }, _) => Err(frame.required_choice_error()),
                }
            }
            (_, RuleResult::Pending { frame, .. }, _) => Err(frame.required_choice_error()),
        }
    }
    /// Run the target gate once for instructions, or complete an ordinary
    /// targetless permanent, then return to the shared supported check boundary.
    pub fn begin_resolution(&self, expected: u64) -> Result<RuleProgress, EffectError> {
        let revision = self.next_revision(expected)?;
        if self.pending_work.is_some() {
            return Err(EffectError::WrongPhase);
        }
        if let Some(entry) = self.window.resolving() {
            if self
                .payloads
                .get(&entry.id())
                .and_then(StackPayload::body)
                .is_some_and(|body| body.target == BodyTarget::None)
                && entry.targets().targets().is_empty()
            {
                let mut next = self.clone();
                next.revision = revision;
                next.pending_work = Some(PendingWork::Resolution(ResolutionCursor {
                    entry: entry.id(),
                    instruction: 0,
                    values: BTreeMap::new(),
                }));
                return next.progress_rules(
                    DamageJournal::default(),
                    vec![],
                    vec![],
                    DrawJournal::default(),
                    RulesJournal::default(),
                );
            }
            if !matches!(
                self.payloads.get(&entry.id()),
                Some(StackPayload::Permanent)
            ) || !entry.targets().targets().is_empty()
            {
                return Err(EffectError::WrongPhase);
            }
            if !matches!(entry.kind(), StackKind::CardSpell(_)) {
                return Err(EffectError::InconsistentProgram);
            }
            let mut next = self.clone();
            next.revision = revision;
            let mut append = RulesJournal::default();
            let movement = next
                .complete_at_cut(
                    entry,
                    crate::occurrence::EntryCompletion::Resolved,
                    Some(ObjectDestination::Battlefield {
                        controller: entry.controller(),
                    }),
                    &mut append,
                )?
                .ok_or(EffectError::InconsistentProgram)?;
            return Ok(RuleProgress::new(
                next,
                RuleResult::Complete {
                    checks: CheckJournal::default(),
                    completion: RuleCompletion::Resolution,
                    declarations: vec![],
                    assignments: vec![],
                    permanent_entry: Some(PermanentEntryRecord {
                        entry: entry.id(),
                        before: movement.before,
                        after: movement.after,
                    }),
                    journal: DamageJournal::default(),
                    draws: DrawJournal::default(),
                },
                append,
            ));
        }
        let entry = self
            .window
            .checking_targets()
            .ok_or(EffectError::WrongPhase)?
            .clone();
        let program = self
            .payloads
            .get(&entry.id())
            .and_then(StackPayload::body)
            .ok_or(EffectError::InconsistentProgram)?;
        let [bound] = entry.targets().targets() else {
            return Err(EffectError::InconsistentProgram);
        };
        let mut next = self.clone();
        next.revision = revision;
        next.window = next.window.check_resolution_targets(
            next.window.revision(),
            entry.id(),
            vec![TargetCheck {
                target: bound.choice().id,
                legal: self.legal_body_target(program, bound.choice().target)?,
            }],
        )?;
        if next
            .window
            .target_assessment()
            .ok_or(EffectError::InconsistentProgram)?
            .permission()
            == ResolutionPermission::AllTargetsIllegal
        {
            let mut append = RulesJournal::default();
            next.complete_at_cut(
                &entry,
                crate::occurrence::EntryCompletion::AllTargetsIllegal,
                next.entry_exit(entry.kind()),
                &mut append,
            )?;
            return Ok(RuleProgress::new(
                next,
                RuleResult::Complete {
                    checks: CheckJournal::default(),
                    completion: RuleCompletion::Resolution,
                    declarations: vec![],
                    assignments: vec![],
                    journal: DamageJournal::default(),
                    permanent_entry: None,
                    draws: DrawJournal::default(),
                },
                append,
            ));
        }
        next.pending_work = Some(PendingWork::Resolution(ResolutionCursor {
            entry: entry.id(),
            instruction: 0,
            values: BTreeMap::new(),
        }));
        next.progress_rules(
            DamageJournal::default(),
            vec![],
            vec![],
            DrawJournal::default(),
            RulesJournal::default(),
        )
    }
    /// Privileged native continuation evidence, not a stepping or wire API.
    pub fn instruction_index(&self) -> Option<usize> {
        self.resolution_state().map(|cursor| cursor.instruction())
    }
    fn current_instruction(&self) -> Result<&Instruction, EffectError> {
        let cursor = self
            .resolution_cursor()
            .ok_or(EffectError::WrongResolutionFrame)?;
        if self.window.resolving().map(|entry| entry.id()) != Some(cursor.entry) {
            return Err(EffectError::WrongResolutionFrame);
        }
        self.payloads
            .get(&cursor.entry)
            .and_then(StackPayload::instructions)
            .and_then(|program| program.get(cursor.instruction))
            .ok_or(EffectError::InconsistentProgram)
    }
    fn advance_instruction(&mut self) -> Result<(), EffectError> {
        let cursor = self
            .resolution_cursor_mut()
            .ok_or(EffectError::WrongResolutionFrame)?;
        cursor.instruction = cursor
            .instruction
            .checked_add(1)
            .ok_or(EffectError::InconsistentProgram)?;
        Ok(())
    }
    fn copy_resolution_frame(&self) -> Result<CopyResolution, EffectError> {
        let entry = self.window.resolving().ok_or(EffectError::WrongPhase)?;
        if self.current_instruction()? != &Instruction::CopySpell {
            return Err(EffectError::WrongResolutionFrame);
        }
        let [bound] = entry.targets().targets() else {
            return Err(EffectError::InconsistentProgram);
        };
        let TargetRef::Stack(original) = bound.choice().target else {
            return Err(EffectError::InconsistentProgram);
        };
        Ok(CopyResolution {
            revision: self.revision,
            entry: entry.id(),
            original,
            chooser: entry.controller(),
            instruction: self
                .instruction_index()
                .ok_or(EffectError::WrongResolutionFrame)?,
        })
    }
    /// Resume the exact current instruction, then continue the rest of its program.
    /// One supplied copy choice; additional choices return an atomic required-input error.
    /// Receipt-dropping direct-fixture convenience; root uses resume_rule.
    pub fn resume_copy_resolution(
        &self,
        expected: u64,
        frame: &CopyResolution,
        actor: SeatId,
        choice: CopyTargetChoice,
    ) -> Result<Self, EffectError> {
        match self
            .resume_rule(
                expected,
                &EffectDecision::CopyTargets(frame.clone()),
                actor,
                DecisionReply::CopyTargets(choice),
                None,
            )?
            .into_parts()
        {
            (state, RuleResult::Complete { .. }, _) => Ok(state),
            (_, RuleResult::Pending { frame, .. }, _) => Err(frame.required_choice_error()),
        }
    }
    /// Resume the exact current owned rules process. A calendar is borrowed only
    /// from the native root; player replies supply no work-completion authority.
    pub fn resume_rule(
        &self,
        expected: u64,
        frame: &EffectDecision,
        actor: SeatId,
        reply: DecisionReply,
        turn: Option<&TurnScheduler>,
    ) -> Result<RuleProgress, EffectError> {
        match frame {
            EffectDecision::ZoneReplacement(frame)
                if matches!(self.pending_work, Some(PendingWork::Checks(_))) =>
            {
                self.resume_check_replacement(expected, frame, actor, reply, turn)
            }
            EffectDecision::LibraryTop(_) => self.resume_library_top(expected, frame, actor, reply),
            EffectDecision::ResolutionGraveyardOrder(_)
                if matches!(self.pending_work, Some(PendingWork::LibraryTop(_))) =>
            {
                self.resume_library_top(expected, frame, actor, reply)
            }
            EffectDecision::ZoneReplacement(_)
                if matches!(self.pending_work, Some(PendingWork::LibraryTop(_))) =>
            {
                self.resume_library_top(expected, frame, actor, reply)
            }
            EffectDecision::ZoneReplacement(_)
            | EffectDecision::ResolutionSelect(_)
            | EffectDecision::ResolutionGraveyardOrder(_) => {
                self.resume_resolution_selection(expected, frame, actor, reply)
            }
            EffectDecision::ReplacementChooser(bound) => {
                self.resume_replacement_chooser(expected, bound, actor, reply)
            }
            EffectDecision::OptionalEffect(bound) => {
                self.resume_optional_effect(expected, bound, actor, reply)
            }
            EffectDecision::ResolutionPayment(bound) => {
                self.resume_resolution_payment(expected, bound, actor, reply, turn)
            }
            EffectDecision::TimestampOrder(bound) => {
                let DecisionReply::TimestampOrder(order) = reply else {
                    return Err(EffectError::WrongDecisionKind);
                };
                let next = self.accept_token_timestamp_order(expected, bound, actor, order)?;
                next.progress_rules(
                    DamageJournal::default(),
                    vec![],
                    vec![],
                    DrawJournal::default(),
                    RulesJournal::default(),
                )
            }
            EffectDecision::CreatureType(bound) => {
                self.ensure_resolution_or_idle()?;
                let DecisionReply::CreatureType(selected) = reply else {
                    return Err(EffectError::WrongDecisionKind);
                };
                let accepted = self.accept_creature_type(expected, bound, actor, selected)?;
                let (next, (), append) = accepted.into_parts();
                next.progress_rules(
                    DamageJournal::default(),
                    vec![],
                    vec![],
                    DrawJournal::default(),
                    append,
                )
            }
            EffectDecision::Checks(checks) => {
                self.resume_checks(expected, checks, actor, reply, turn)
            }
            EffectDecision::Triggers(order) => {
                self.resume_trigger_order(expected, order, actor, reply, turn)
            }
            EffectDecision::TriggerTarget(frame) => {
                self.resume_trigger_target(expected, frame, actor, reply, turn)
            }
            EffectDecision::Combat(combat) => {
                let turn = turn.ok_or(EffectError::WrongPhase)?;
                match (combat, reply) {
                    (
                        CombatDecision::DamageAssignments(bound),
                        DecisionReply::AssignCombatDamage(rows),
                    ) => self.resume_combat_damage_assignments(expected, bound, actor, rows, turn),
                    (CombatDecision::DamageAssignments(_), _) => {
                        Err(EffectError::WrongDecisionKind)
                    }
                    (_, other) => self.resume_combat(expected, combat, actor, other, turn),
                }
            }
            EffectDecision::Damage(damage) => {
                self.resume_damage(expected, damage, actor, reply, turn)
            }
            EffectDecision::CopyTargets(bound) => {
                self.ensure_resolution_or_idle()?;
                let DecisionReply::CopyTargets(choice) = reply else {
                    return Err(EffectError::WrongDecisionKind);
                };
                let revision = self.next_revision(expected)?;
                if &self.copy_resolution_frame()? != bound {
                    return Err(EffectError::WrongResolutionFrame);
                }
                if actor != bound.chooser {
                    return Err(EffectError::WrongChooser);
                }
                let entry = self
                    .window
                    .resolving()
                    .ok_or(EffectError::WrongPhase)?
                    .clone();
                let mut next = self.clone();
                next.revision = revision;
                let mut journal = DamageJournal::default();
                let mut append = RulesJournal::default();
                next.apply_instruction(
                    entry,
                    Instruction::CopySpell,
                    Some((actor, choice)),
                    &mut journal,
                    &mut append,
                )?;
                next.advance_instruction()?;
                next.progress_rules(journal, vec![], vec![], DrawJournal::default(), append)
            }
        }
    }
    /// One iterative driver serves resolution and combat-origin damage. Returning
    /// from one damage instruction never recursively completes the whole program.
    fn progress_rules(
        mut self,
        mut journal: DamageJournal,
        declarations: Vec<CombatDeclarationRecord>,
        assignments: Vec<CombatAssignmentRecord>,
        mut draws: DrawJournal,
        mut append: RulesJournal,
    ) -> Result<RuleProgress, EffectError> {
        loop {
            if matches!(self.pending_work, Some(PendingWork::LibraryTop(_))) {
                if let Some(frame) = self.progress_library_top(&mut append)? {
                    return Ok(RuleProgress::new(
                        self,
                        RuleResult::Pending {
                            checks: CheckJournal::default(),
                            frame,
                            journal,
                            declarations,
                            assignments,
                            draws,
                        },
                        append,
                    ));
                }
                continue;
            }
            if matches!(self.pending_work, Some(PendingWork::ResolutionSelection(_))) {
                if let Some(frame) = self.progress_resolution_selection(&mut append)? {
                    return Ok(RuleProgress::new(
                        self,
                        RuleResult::Pending {
                            checks: CheckJournal::default(),
                            frame,
                            journal,
                            declarations,
                            assignments,
                            draws,
                        },
                        append,
                    ));
                }
                continue;
            }
            if matches!(self.pending_work, Some(PendingWork::TokenEntry(_))) {
                if let Some(frame) = self.progress_token_entries(&mut append)? {
                    return Ok(RuleProgress::new(
                        self,
                        RuleResult::Pending {
                            checks: CheckJournal::default(),
                            frame: EffectDecision::TimestampOrder(frame),
                            journal,
                            declarations,
                            assignments,
                            draws,
                        },
                        append,
                    ));
                }
                continue;
            }
            if matches!(self.pending_work, Some(PendingWork::Damage(_))) {
                match self.progress_damage(&mut journal, &mut append)? {
                    DamageProgressStep::Pending(frame) => {
                        return Ok(RuleProgress::new(
                            self,
                            RuleResult::Pending {
                                checks: CheckJournal::default(),
                                frame: EffectDecision::Damage(frame),
                                journal,
                                declarations,
                                assignments,
                                draws,
                            },
                            append,
                        ));
                    }
                    DamageProgressStep::ResolutionContinued => {
                        #[cfg(test)]
                        characteristic_observations::damage_instruction_completed(&self);
                        continue;
                    }
                    DamageProgressStep::CombatComplete => {
                        return Ok(RuleProgress::new(
                            self,
                            RuleResult::Complete {
                                checks: CheckJournal::default(),
                                completion: RuleCompletion::TurnWork(WorkCompleted::CombatDamage {
                                    additional_step_required: false,
                                }),
                                journal,
                                declarations,
                                assignments,
                                permanent_entry: None,
                                draws,
                            },
                            append,
                        ));
                    }
                }
            }
            let entry = self
                .window
                .resolving()
                .ok_or(EffectError::WrongPhase)?
                .clone();
            let cursor = self
                .resolution_cursor()
                .ok_or(EffectError::WrongResolutionFrame)?;
            if cursor.entry != entry.id() {
                return Err(EffectError::WrongResolutionFrame);
            }
            let index = cursor.instruction;
            let program = self
                .payloads
                .get(&entry.id())
                .and_then(StackPayload::instructions)
                .ok_or(EffectError::InconsistentProgram)?;
            if index == program.len() {
                self.complete_at_cut(
                    &entry,
                    crate::occurrence::EntryCompletion::Resolved,
                    self.entry_exit(entry.kind()),
                    &mut append,
                )?;
                self.pending_work = None;
                return Ok(RuleProgress::new(
                    self,
                    RuleResult::Complete {
                        checks: CheckJournal::default(),
                        completion: RuleCompletion::Resolution,
                        journal,
                        declarations,
                        assignments,
                        permanent_entry: None,
                        draws,
                    },
                    append,
                ));
            }
            let instruction = program
                .get(index)
                .ok_or(EffectError::InconsistentProgram)?
                .clone();
            #[cfg(test)]
            characteristic_observations::instruction(
                &self,
                "Before",
                entry.id(),
                index,
                Some(&instruction),
            );
            if self.begin_library_top(&instruction)? {
                continue;
            }
            if self.begin_resolution_selection(&instruction)? {
                continue;
            }
            if self.begin_resolution_choice(&instruction, &mut append)? {
                let Some(frame) = self.pending_resolution_decision() else {
                    // A departed payer follows the unpaid branch without
                    // allocating or exposing a response for an absent seat.
                    continue;
                };
                return Ok(RuleProgress::new(
                    self,
                    RuleResult::Pending {
                        checks: CheckJournal::default(),
                        frame,
                        journal,
                        declarations,
                        assignments,
                        draws,
                    },
                    append,
                ));
            }
            if self.progress_control(&entry, &instruction, &mut append)? {
                #[cfg(test)]
                characteristic_observations::instruction(
                    &self,
                    "Completed",
                    entry.id(),
                    index,
                    None,
                );
                continue;
            }
            if matches!(instruction, Instruction::ChooseCreatureType { .. }) {
                let frame = EffectDecision::CreatureType(self.creature_type_frame()?);
                return Ok(RuleProgress::new(
                    self,
                    RuleResult::Pending {
                        checks: CheckJournal::default(),
                        frame,
                        journal,
                        declarations,
                        assignments,
                        draws,
                    },
                    append,
                ));
            }
            if let Instruction::DrawController { amount } = instruction {
                let amount = self.resolve_instruction_amount(entry.controller(), &amount)?;
                draws.append(self.draw_at_cuts(
                    entry.controller(),
                    amount,
                    DrawOrigin::Instruction {
                        entry: entry.id(),
                        instruction: index,
                    },
                    &mut append,
                )?);
                self.advance_instruction()?;
                #[cfg(test)]
                characteristic_observations::instruction(
                    &self,
                    "Completed",
                    entry.id(),
                    index,
                    None,
                );
                continue;
            }
            if let Instruction::AdjustControllerLife { adjustment } = instruction {
                self.adjust_life_at_cut(
                    entry.id(),
                    index,
                    entry.controller(),
                    &adjustment,
                    &mut append,
                )?;
                self.advance_instruction()?;
                #[cfg(test)]
                characteristic_observations::instruction(
                    &self,
                    "Completed",
                    entry.id(),
                    index,
                    None,
                );
                continue;
            }
            if let Instruction::CreateTokens { creations } = instruction {
                self.begin_token_entries(&creations)?;
                continue;
            }
            let [bound] = entry.targets().targets() else {
                return Err(EffectError::InconsistentProgram);
            };
            let target = bound.choice().target;
            if !self.operation_available(&instruction, target)? {
                self.advance_instruction()?;
                #[cfg(test)]
                characteristic_observations::instruction(
                    &self,
                    "SkippedUnavailable",
                    entry.id(),
                    index,
                    None,
                );
                continue;
            }
            match instruction {
                Instruction::CopySpell => {
                    let TargetRef::Stack(original) = target else {
                        return Err(EffectError::InconsistentProgram);
                    };
                    let body_target = self
                        .stack_body(original)
                        .ok_or(EffectError::InconsistentProgram)?
                        .target;
                    if body_target == BodyTarget::None {
                        #[cfg(test)]
                        let copying_entry = entry.id();
                        let controller = entry.controller();
                        self.apply_instruction(
                            entry,
                            Instruction::CopySpell,
                            Some((controller, CopyTargetChoice::Retain)),
                            &mut journal,
                            &mut append,
                        )?;
                        self.advance_instruction()?;
                        #[cfg(test)]
                        characteristic_observations::instruction(
                            &self,
                            "Completed",
                            copying_entry,
                            index,
                            None,
                        );
                        continue;
                    }
                    let frame = EffectDecision::CopyTargets(self.copy_resolution_frame()?);
                    return Ok(RuleProgress::new(
                        self,
                        RuleResult::Pending {
                            checks: CheckJournal::default(),
                            frame,
                            journal,
                            declarations,
                            assignments,
                            draws,
                        },
                        append,
                    ));
                }
                Instruction::Damage { amount } => {
                    let target = match target {
                        TargetRef::Object(target) => DamageRecipient::Creature(target),
                        TargetRef::Player(player) => DamageRecipient::Player(player),
                        TargetRef::Stack(_) => return Err(EffectError::InconsistentProgram),
                    };
                    let source = self.resolution_damage_source(&entry)?;
                    let origin = DamageOrigin::Resolution {
                        entry: entry.id(),
                        instruction: index,
                    };
                    self.install_damage_batch(
                        origin,
                        DamageKind::Noncombat,
                        vec![DamageInput {
                            source,
                            target,
                            amount,
                        }],
                        &mut journal,
                    )?;
                }
                Instruction::AdjustTargetLife { adjustment } => {
                    let TargetRef::Player(player) = target else {
                        return Err(EffectError::InconsistentProgram);
                    };
                    self.adjust_life_at_cut(entry.id(), index, player, &adjustment, &mut append)?;
                    self.advance_instruction()?;
                    #[cfg(test)]
                    characteristic_observations::instruction(
                        &self,
                        "Completed",
                        entry.id(),
                        index,
                        None,
                    );
                }
                other => {
                    #[cfg(test)]
                    let observed_entry = entry.id();
                    self.apply_instruction(entry, other, None, &mut journal, &mut append)?;
                    self.advance_instruction()?;
                    #[cfg(test)]
                    characteristic_observations::instruction(
                        &self,
                        "Completed",
                        observed_entry,
                        index,
                        None,
                    );
                }
            }
        }
    }
    fn apply_instruction(
        &mut self,
        entry: crate::stack_window::StackEntry,
        instruction: Instruction,
        copy_choice: Option<(SeatId, CopyTargetChoice)>,
        journal: &mut DamageJournal,
        append: &mut RulesJournal,
    ) -> Result<(), EffectError> {
        use crate::occurrence::{EventCause, RulesOccurrence, ZoneReason};
        let index = self
            .instruction_index()
            .ok_or(EffectError::WrongResolutionFrame)?;
        let [bound] = entry.targets().targets() else {
            return Err(EffectError::InconsistentProgram);
        };
        let choice = bound.choice();
        let target = choice.target;
        let before = self.rules_snapshot(None)?;
        enum InstructionCut {
            Ready(RulesOccurrence),
            Exit {
                exit: crate::stack_window::StackExit,
                reason: ZoneReason,
            },
            Continuous(ResolvedContinuousEffect),
            Counters {
                object: ObjectId,
                before: PowerToughnessCounters,
                added: PowerToughnessCounters,
                after: PowerToughnessCounters,
            },
        }
        let mut cause = EventCause::Instruction {
            entry: entry.id(),
            instruction: index,
        };
        let receipt = match instruction {
            Instruction::Sacrifice { .. } | Instruction::Discard { .. } => {
                return Err(EffectError::InconsistentProgram);
            }
            Instruction::OptionalEffect { .. }
            | Instruction::OfferManaPayment { .. }
            | Instruction::BranchIfFalse { .. }
            | Instruction::Jump { .. }
            | Instruction::Damage { .. }
            | Instruction::CreateTokens { .. }
            | Instruction::Scry { .. }
            | Instruction::Surveil { .. }
            | Instruction::Mill { .. }
            | Instruction::DrawController { .. }
            | Instruction::AdjustControllerLife { .. }
            | Instruction::AdjustTargetLife { .. }
            | Instruction::ChooseCreatureType { .. } => {
                return Err(EffectError::InconsistentProgram);
            }
            Instruction::CreateDamageMultiplier { factor } => {
                let TargetRef::Object(target) = target else {
                    return Err(EffectError::InconsistentProgram);
                };
                let multiplier = self.damage_state.add_multiplier(
                    entry.id(),
                    DamageEffectTarget::Permanent(target),
                    factor,
                    DamageSourceFilter::Any,
                )?;
                let created = DamageEffectCreation::Multiplier {
                    multiplier,
                    instruction: index,
                };
                journal.created.push(created.clone());
                InstructionCut::Ready(RulesOccurrence::DamageEffectCreated(created))
            }
            Instruction::CreatePreventionShield { amount } => {
                let TargetRef::Object(target) = target else {
                    return Err(EffectError::InconsistentProgram);
                };
                let shield = self.damage_state.add_shield(
                    entry.id(),
                    DamageEffectTarget::Permanent(target),
                    amount,
                )?;
                let created = DamageEffectCreation::Prevention {
                    shield,
                    instruction: index,
                };
                journal.created.push(created.clone());
                InstructionCut::Ready(RulesOccurrence::DamageEffectCreated(created))
            }
            Instruction::CounterSpell => {
                let TargetRef::Stack(countered) = target else {
                    return Err(EffectError::InconsistentProgram);
                };
                let (window, exit) = self.window.record_targeted_resolution_move_with_exit(
                    self.window.revision(),
                    choice.id,
                    ObjectDestination::Graveyard,
                )?;
                self.window = window;
                cause = EventCause::CounterSpell {
                    entry: entry.id(),
                    instruction: index,
                    target: countered,
                };
                InstructionCut::Exit {
                    exit,
                    reason: ZoneReason::Countered,
                }
            }
            Instruction::CopySpell => {
                let (actor, selected) = copy_choice.ok_or(EffectError::CopyChoiceRequired)?;
                if actor != entry.controller() {
                    return Err(EffectError::WrongChooser);
                }
                let TargetRef::Stack(original) = target else {
                    return Err(EffectError::InconsistentProgram);
                };
                let original_entry = self
                    .window
                    .entries()
                    .iter()
                    .find(|item| item.id() == original)
                    .ok_or(EffectError::InconsistentProgram)?;
                let body = self
                    .payloads
                    .get(&original)
                    .and_then(StackPayload::body)
                    .ok_or(EffectError::InconsistentProgram)?
                    .clone();
                let traits = self
                    .stack_copiable_traits(original)
                    .ok_or(EffectError::InconsistentProgram)?;
                let copiable_traits = CopiableSpellTraits {
                    names: traits.names.clone(),
                    supertypes: traits.supertypes.clone(),
                    card_types: traits.card_types.clone(),
                    creature_subtypes: traits.creature_subtypes.clone(),
                    triggered: traits.triggered.to_vec(),
                };
                let changes = match selected {
                    CopyTargetChoice::Retain => Vec::new(),
                    CopyTargetChoice::Change(target) => {
                        if !self.legal_body_target(&body, target)? {
                            return Err(EffectError::IllegalTarget);
                        }
                        let [bound] = original_entry.targets().targets() else {
                            return Err(EffectError::InconsistentProgram);
                        };
                        let mut replacement = bound.choice();
                        replacement.target = target;
                        vec![replacement]
                    }
                };
                self.window = self.window.record_spell_copy(
                    self.window.revision(),
                    original,
                    actor,
                    changes,
                )?;
                let copy = self
                    .window
                    .entries()
                    .last()
                    .ok_or(EffectError::InconsistentProgram)?;
                let id = copy.id();
                let targets = copy
                    .targets()
                    .targets()
                    .iter()
                    .map(|bound| bound.choice())
                    .collect();
                self.payloads.insert(
                    id,
                    StackPayload::from_body(body, Some(copiable_traits.clone())),
                );
                InstructionCut::Ready(RulesOccurrence::CopyCreated {
                    copiable_card_types: copiable_traits.card_types,
                    copiable_creature_subtypes: copiable_traits.creature_subtypes,
                    copiable_names: copiable_traits.names,
                    copiable_supertypes: copiable_traits.supertypes,
                    copiable_triggered: copiable_traits.triggered,
                    entry: id,
                    original,
                    controller: actor,
                    targets,
                })
            }
            Instruction::CreateContinuousEffect { parts, duration } => {
                let TargetRef::Object(target) = target else {
                    return Err(EffectError::InconsistentProgram);
                };
                let locals = self
                    .resolution_state()
                    .ok_or(EffectError::WrongResolutionFrame)?;
                let owner = self
                    .window
                    .cards()
                    .get(target)
                    .ok_or(EffectError::InconsistentProgram)?
                    .owner();
                let parts = parts.materialize(locals.locals(), entry.controller(), owner)?;
                InstructionCut::Continuous(self.characteristics.add_effect(
                    entry.id(),
                    target,
                    parts,
                    duration,
                )?)
            }
            Instruction::CopyTargetToSource {
                duration,
                power_toughness,
            } => {
                let (StackKind::ActivatedAbility { source }, TargetRef::Object(donor)) =
                    (entry.kind(), target)
                else {
                    return Err(EffectError::InconsistentProgram);
                };
                // A departed or replaced receiver cannot be changed. Its
                // already captured body still resolves subsequent instructions.
                let receiver = source.id();
                if !self
                    .window
                    .cards()
                    .get(receiver)
                    .is_some_and(|object| object.zone() == ObjectZone::Battlefield)
                {
                    return Ok(());
                }
                let mut captured = self
                    .copiable_program(donor)
                    .ok_or(EffectError::InconsistentProgram)?
                    .clone();
                let ProgramKind::Permanent(permanent) = &mut captured.kind else {
                    return Err(EffectError::InconsistentProgram);
                };
                if let Some(pair) = power_toughness {
                    permanent.printed_pt = Some(pair.into());
                    // CR 707.9d excludes the whole defining ability, including
                    // its other parts, rather than editing just its PT part.
                    if matches!(&captured.continuous,
                        Some(crate::characteristics::StaticContinuousAbility::CharacteristicDefining { parts, .. })
                        if parts.0.iter().any(|part| matches!(part, crate::characteristics::DefiningPart::PowerToughness { .. })))
                    {
                        captured.continuous = None;
                    }
                }
                InstructionCut::Continuous(self.characteristics.add_effect(
                    entry.id(),
                    receiver,
                    crate::characteristics::ResolvedParts(vec![
                        crate::characteristics::ResolvedPart::Copy(Box::new(captured)),
                    ]),
                    duration,
                )?)
            }
            Instruction::PutPowerToughnessCounters { counters } => {
                let TargetRef::Object(target) = target else {
                    return Err(EffectError::InconsistentProgram);
                };
                let previous = self.characteristics.counters(target);
                self.characteristics.place_counters(target, &counters)?;
                InstructionCut::Counters {
                    object: target,
                    before: previous,
                    added: counters,
                    after: self.characteristics.counters(target),
                }
            }
            Instruction::ReturnToOwnersHand => {
                let (window, exit) = self.window.record_targeted_resolution_move_with_exit(
                    self.window.revision(),
                    choice.id,
                    ObjectDestination::Hand,
                )?;
                self.window = window;
                InstructionCut::Exit {
                    exit,
                    reason: ZoneReason::ReturnToOwnersHand,
                }
            }
        };
        self.emit_from(
            cause,
            &before,
            None,
            append,
            |previous, current, controllers| {
                Ok(vec![match receipt {
                    InstructionCut::Ready(value) => value,
                    InstructionCut::Exit { exit, reason } => {
                        exit_occurrence_at(previous, current, exit, reason, controllers)?
                    }
                    InstructionCut::Continuous(effect) => {
                        let appearances = characteristic_appearance_changes(
                            previous,
                            current,
                            effect.affected.iter().copied(),
                        )?;
                        RulesOccurrence::ContinuousEffectCreated {
                            effect,
                            appearances,
                        }
                    }
                    InstructionCut::Counters {
                        object,
                        before,
                        added,
                        after,
                    } => RulesOccurrence::CountersPlaced {
                        object,
                        before,
                        added,
                        after,
                        before_appearance: Box::new(
                            previous
                                .appearance(object)
                                .ok_or(EffectError::InconsistentProgram)?,
                        ),
                        after_appearance: Box::new(
                            current
                                .appearance(object)
                                .ok_or(EffectError::InconsistentProgram)?,
                        ),
                    },
                }])
            },
        )?;
        Ok(())
    }
    fn entry_exit(&self, kind: &StackKind) -> Option<ObjectDestination> {
        match kind {
            StackKind::CardSpell(object) => self
                .window
                .cards()
                .get(*object)
                .map(|_| ObjectDestination::Graveyard),
            StackKind::SpellCopy { .. } => Some(ObjectDestination::Graveyard),
            StackKind::ActivatedAbility { .. } | StackKind::TriggeredAbility { .. } => None,
        }
    }
    fn legal_body_target(
        &self,
        body: &ExecutableBody,
        target: TargetRef,
    ) -> Result<bool, EffectError> {
        let evaluated = self.evaluated_characteristics()?;
        Ok(self.legal_body_target_in(body, target, &evaluated))
    }
    fn legal_body_target_in(
        &self,
        body: &ExecutableBody,
        target: TargetRef,
        evaluated: &EvaluatedCharacteristics,
    ) -> bool {
        match body.target {
            BodyTarget::None => false,
            BodyTarget::SingleCreature => matches!(target, TargetRef::Object(id)
                if self.window.cards().get(id).is_some_and(|card| card.zone() == ObjectZone::Battlefield)
                    && evaluated.get(id).is_some_and(ObjectCharacteristics::is_creature)),
            BodyTarget::SinglePlayer => {
                matches!(target, TargetRef::Player(player) if self.window.participants().contains(&player))
            }
            BodyTarget::SingleCreatureOrPlayer => match target {
                TargetRef::Player(player) => self.window.participants().contains(&player),
                TargetRef::Object(id) => {
                    self.window
                        .cards()
                        .get(id)
                        .is_some_and(|card| card.zone() == ObjectZone::Battlefield)
                        && evaluated
                            .get(id)
                            .is_some_and(ObjectCharacteristics::is_creature)
                }
                TargetRef::Stack(_) => false,
            },
            BodyTarget::SinglePermanent => matches!(target, TargetRef::Object(id)
                if self.window.cards().get(id).is_some_and(|card| card.zone() == ObjectZone::Battlefield)),
            BodyTarget::SingleSpell(filter) => match target {
                TargetRef::Stack(id) => {
                    self.window.entries().iter().any(|entry| {
                        entry.id() == id
                            && matches!(
                                entry.kind(),
                                StackKind::CardSpell(_) | StackKind::SpellCopy { .. }
                            )
                    }) && (filter == SpellTargetFilter::AnySpell || self.copyable_payload(id))
                }
                _ => false,
            },
        }
    }
    /// Enumerate once against the same current view and predicate as normal targets.
    fn trigger_target_domain(&self, body: &ExecutableBody) -> Result<Vec<TargetRef>, EffectError> {
        let evaluated = self.evaluated_characteristics()?;
        let candidates = self
            .window
            .cards()
            .iter()
            .map(|card| TargetRef::Object(card.id()))
            .chain(
                self.window
                    .participants()
                    .iter()
                    .copied()
                    .map(TargetRef::Player),
            )
            .chain(
                self.window
                    .entries()
                    .iter()
                    .map(|entry| TargetRef::Stack(entry.id())),
            );
        Ok(candidates
            .filter(|target| self.legal_body_target_in(body, *target, &evaluated))
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect())
    }
    fn copyable_payload(&self, id: StackEntryId) -> bool {
        self.payloads
            .get(&id)
            .and_then(StackPayload::body)
            .is_some()
    }
    /// An instruction uses its already bound exact incarnation; this is not
    /// another check of the body's original target restriction.
    fn operation_available(
        &self,
        instruction: &Instruction,
        target: TargetRef,
    ) -> Result<bool, EffectError> {
        let evaluated = self.evaluated_characteristics()?;
        Ok(match instruction {
            Instruction::Sacrifice { .. } | Instruction::Discard { .. } => {
                return Err(EffectError::InconsistentProgram);
            }
            Instruction::AdjustTargetLife { .. } => match target {
                TargetRef::Player(player) => self.window.participants().contains(&player),
                TargetRef::Object(_) | TargetRef::Stack(_) => false,
            },
            Instruction::CounterSpell => match target {
                TargetRef::Stack(id) => self.window.entries().iter().any(|entry| {
                    entry.id() == id
                        && matches!(
                            entry.kind(),
                            StackKind::CardSpell(_) | StackKind::SpellCopy { .. }
                        )
                }),
                TargetRef::Object(_) | TargetRef::Player(_) => false,
            },
            Instruction::CopySpell => match target {
                TargetRef::Stack(id) => {
                    self.copyable_payload(id)
                        && self.window.entries().iter().any(|entry| {
                            entry.id() == id
                                && matches!(
                                    entry.kind(),
                                    StackKind::CardSpell(_) | StackKind::SpellCopy { .. }
                                )
                        })
                }
                TargetRef::Object(_) | TargetRef::Player(_) => false,
            },
            Instruction::Damage { .. } => match target {
                TargetRef::Player(player) => self.window.participants().contains(&player),
                TargetRef::Object(card) => {
                    self.window
                        .cards()
                        .get(card)
                        .is_some_and(|value| value.zone() == ObjectZone::Battlefield)
                        && evaluated
                            .get(card)
                            .is_some_and(ObjectCharacteristics::is_creature)
                }
                TargetRef::Stack(_) => false,
            },
            Instruction::CreatePreventionShield { .. }
            | Instruction::CreateDamageMultiplier { .. }
            | Instruction::ReturnToOwnersHand
            | Instruction::CreateContinuousEffect { .. }
            | Instruction::CopyTargetToSource { .. }
            | Instruction::PutPowerToughnessCounters { .. } => match target {
                TargetRef::Object(card) => self
                    .window
                    .cards()
                    .get(card)
                    .is_some_and(|value| value.zone() == ObjectZone::Battlefield),
                TargetRef::Player(_) | TargetRef::Stack(_) => false,
            },
            // The iterative driver owns these paths before target availability.
            Instruction::OptionalEffect { .. }
            | Instruction::OfferManaPayment { .. }
            | Instruction::BranchIfFalse { .. }
            | Instruction::Jump { .. }
            | Instruction::CreateTokens { .. }
            | Instruction::Scry { .. }
            | Instruction::Surveil { .. }
            | Instruction::Mill { .. }
            | Instruction::DrawController { .. }
            | Instruction::ChooseCreatureType { .. }
            | Instruction::AdjustControllerLife { .. } => false,
        })
    }

    fn prune_payloads(&mut self) {
        let live: BTreeSet<_> = self
            .window
            .entries()
            .iter()
            .map(|entry| entry.id())
            .collect();
        self.payloads.retain(|id, _| live.contains(id));
    }
    fn prune_non_battlefield_state(&mut self) {
        let current: BTreeSet<_> = self
            .window
            .cards()
            .iter()
            .filter(|card| card.zone() == ObjectZone::Battlefield)
            .map(|card| card.id())
            .collect();
        self.damage.retain(|object, _| current.contains(object));
        self.characteristics.retain_battlefield(&current);
        self.damage_state.retain_recipients(&current);
    }
}

#[cfg(test)]
mod revision_tests {
    use super::*;
    use crate::activated::SourceCost;
    #[test]
    fn failed_sba_work_never_publishes_a_loss_or_discards_its_pending_history() {
        let a = SeatId(0);
        let mut state = EffectWindow::new(GameSeed::with_uniform_life(
            vec![a, SeatId(1)],
            a,
            vec![SeedCard {
                card: CardId(200),
                owner: a,
                destination: ObjectDestination::Battlefield { controller: a },
                program: Program::creature(
                    None,
                    1.into(),
                    0.into(),
                    std::collections::BTreeSet::new(),
                ),
            }],
            vec![CardId(200)],
            vec![],
            20.into(),
            CombatRules::UnrestrictedFreeForAll,
            crate::test_support::creature_types::synthetic_vocabulary(),
            std::sync::Arc::new(spellwake_contracts::names::NameRules::literal_names()),
        ))
        .unwrap();
        state = state
            .enter_turn_step(state.revision(), a, false, false)
            .unwrap()
            .into_state();
        state = state
            .draw_cards(state.revision(), a, 1.into())
            .unwrap()
            .into_state();
        state = state
            .enter_turn_step(state.revision(), a, true, false)
            .unwrap()
            .into_state();
        state.window.set_test_next_object_identity(u64::MAX);
        let before = state.clone();
        assert!(state.settle_checks(state.revision()).is_err());
        assert_eq!(state, before);
        assert!(state.failed_draws.contains(&a));
        assert!(state.state_based_losses.is_empty());
    }

    #[test]
    fn failed_draw_survives_refill_and_waits_until_resolution_finishes() {
        let a = SeatId(0);
        let b = SeatId(1);
        let mut effect = EffectWindow::new(GameSeed::with_uniform_life(
            vec![a, b],
            a,
            vec![
                SeedCard {
                    card: CardId(1),
                    owner: a,
                    destination: ObjectDestination::Hand,
                    program: Program::instant(
                        ManaCost::free(),
                        ExecutableBody::new(
                            BodyTarget::SingleCreature,
                            vec![Instruction::Damage { amount: 1.into() }],
                        ),
                    ),
                },
                SeedCard {
                    card: CardId(2),
                    owner: a,
                    destination: ObjectDestination::Hand,
                    program: Program::creature(
                        None,
                        1.into(),
                        1.into(),
                        std::collections::BTreeSet::new(),
                    ),
                },
            ],
            vec![],
            vec![],
            20.into(),
            CombatRules::UnrestrictedFreeForAll,
            crate::test_support::creature_types::synthetic_vocabulary(),
            std::sync::Arc::new(spellwake_contracts::names::NameRules::literal_names()),
        ))
        .unwrap();
        effect = effect.settle_checks(effect.revision()).unwrap();
        let source = effect.window.cards().current_identity(CardId(1)).unwrap();
        effect.window = effect
            .window
            .record_card_spell(effect.window.revision(), a, source)
            .unwrap();
        effect.window = effect
            .window
            .finish_checks(effect.window.revision())
            .unwrap();
        effect.window = effect.window.pass(effect.window.revision(), a).unwrap();
        effect.window = effect
            .window
            .finish_checks(effect.window.revision())
            .unwrap();
        effect.window = effect.window.pass(effect.window.revision(), b).unwrap();
        let (mut drawn, journal, _) = effect
            .draw_cards(effect.revision(), a, 1.into())
            .unwrap()
            .into_parts();
        assert!(matches!(
            journal.events.as_slice(),
            [DrawEvent::EmptyLibrary { .. }]
        ));
        assert!(drawn.state_based_losses.is_empty());
        assert!(drawn.failed_draws.contains(&a));
        let refill = drawn.window.cards().current_identity(CardId(2)).unwrap();
        drawn.window = drawn
            .window
            .record_resolution_move(
                drawn.window.revision(),
                refill,
                ObjectDestination::LibraryTop,
            )
            .unwrap();
        assert_eq!(drawn.window.cards().library(a).len(), 1);
        assert!(drawn.state_based_losses.is_empty());
        let copy = drawn.clone();
        assert!(copy.failed_draws.contains(&a));
        let entry = drawn.window.resolving().unwrap().id();
        drawn.window = drawn
            .window
            .finish_resolution(
                drawn.window.revision(),
                entry,
                Some(ObjectDestination::Graveyard),
            )
            .unwrap();
        let checked = drawn.settle_checks(drawn.revision()).unwrap();
        assert!(checked.failed_draws.is_empty());
        assert!(checked.state_based_losses.contains(&a));
        assert!(!matches!(
            checked.window.phase(),
            WindowPhase::Priority(PriorityPhase::Ready { .. })
        ));
    }

    #[test]
    fn pending_choice_rejects_drawing_and_keeps_paid_state() {
        let state = color_window();
        let source = state.window.cards().current_identity(CardId(1)).unwrap();
        let (state, ManaActivationResult::Pending { .. }, _) = state
            .begin_mana_activation(
                state.revision(),
                SeatId(0),
                source,
                crate::mana::ManaAbilityRef::Printed(ManaAbilityId(7)),
            )
            .unwrap()
            .into_parts()
        else {
            panic!("expected actual color choice");
        };
        let before = state.clone();
        assert_eq!(
            state.draw_cards(state.revision(), SeatId(0), 1.into()),
            Err(EffectError::WrongPhase)
        );
        assert_eq!(state, before);
    }

    #[test]
    fn untap_work_defers_supported_state_checks_until_upkeep_priority() {
        let a = SeatId(0);
        let zero = EffectWindow::new(GameSeed::with_uniform_life(
            vec![a, SeatId(1)],
            a,
            vec![SeedCard {
                card: CardId(777),
                owner: a,
                destination: ObjectDestination::Battlefield { controller: a },
                program: Program::creature(
                    None,
                    1.into(),
                    0.into(),
                    std::collections::BTreeSet::new(),
                ),
            }],
            vec![CardId(777)],
            vec![],
            20.into(),
            CombatRules::UnrestrictedFreeForAll,
            crate::test_support::creature_types::synthetic_vocabulary(),
            std::sync::Arc::new(spellwake_contracts::names::NameRules::literal_names()),
        ))
        .unwrap();
        let work = zero
            .enter_turn_step(zero.revision(), a, false, false)
            .unwrap()
            .into_state();
        let original = work.window.cards().current_identity(CardId(777)).unwrap();
        assert_eq!(
            work.window.cards().get(original).unwrap().zone(),
            ObjectZone::Battlefield
        );
        assert!(matches!(
            work.settle_checks(work.revision()),
            Err(EffectError::WrongPhase)
        ));
        let untapped = work.untap_turn(work.revision()).unwrap().into_state();
        assert_eq!(
            untapped.window.cards().current_identity(CardId(777)),
            Some(original)
        );
        assert_eq!(untapped.window.phase(), WindowPhase::TurnWork);
        let upkeep = untapped
            .enter_turn_step(untapped.revision(), a, true, true)
            .unwrap()
            .into_state();
        let checked = upkeep.settle_checks(upkeep.revision()).unwrap();
        assert_eq!(checked.window.cards().get(original), None);
        let current = checked
            .window
            .cards()
            .current_identity(CardId(777))
            .unwrap();
        assert_eq!(
            checked.window.cards().get(current).unwrap().zone(),
            ObjectZone::Graveyard(a)
        );
    }

    #[test]
    fn lifecycle_cannot_bypass_a_paid_mana_choice() {
        let state = color_window();
        let source = state.window.cards().current_identity(CardId(1)).unwrap();
        let (pending, ManaActivationResult::Pending { .. }, _) = state
            .begin_mana_activation(
                state.revision(),
                SeatId(0),
                source,
                crate::mana::ManaAbilityRef::Printed(ManaAbilityId(7)),
            )
            .unwrap()
            .into_parts()
        else {
            panic!("expected owned color work");
        };
        let before = pending.clone();
        assert_eq!(
            pending.enter_turn_step(pending.revision(), SeatId(1), true, true),
            Err(EffectError::WrongPhase)
        );
        assert_eq!(
            pending.untap_turn(pending.revision()),
            Err(EffectError::WrongPhase)
        );
        assert_eq!(
            pending.begin_cleanup(
                pending.revision(),
                &TurnScheduler::new(
                    crate::turn::TurnStart {
                        number: 1,
                        step: crate::turn::Step::Cleanup
                    },
                    false
                )
                .unwrap()
            ),
            Err(EffectError::WrongPhase)
        );
        assert_eq!(pending, before);
    }
    fn activation_window(source_cost: SourceCost) -> EffectWindow {
        let a = SeatId(0);
        let b = SeatId(1);
        let state = EffectWindow::new(GameSeed::with_uniform_life(
            vec![a, b],
            a,
            vec![
                SeedCard {
                    card: CardId(1),
                    owner: b,
                    destination: ObjectDestination::Battlefield { controller: a },
                    program: Program::artifact(
                        None,
                        vec![ActivatedAbility {
                            id: ActivatedAbilityId(7),
                            mana_cost: ManaCost::one(ManaType::Red),
                            source_cost,
                            body: ExecutableBody::new(
                                BodyTarget::SingleCreature,
                                vec![Instruction::Damage { amount: 1.into() }],
                            ),
                        }],
                    ),
                },
                SeedCard {
                    card: CardId(2),
                    owner: b,
                    destination: ObjectDestination::Battlefield { controller: b },
                    program: Program::creature(
                        None,
                        3.into(),
                        5.into(),
                        std::collections::BTreeSet::new(),
                    ),
                },
            ],
            vec![CardId(1), CardId(2)],
            vec![
                FloatingMana {
                    player: a,
                    mana_type: ManaType::Red,
                    amount: 7.into(),
                },
                FloatingMana {
                    player: b,
                    mana_type: ManaType::Blue,
                    amount: 11.into(),
                },
            ],
            20.into(),
            CombatRules::UnrestrictedFreeForAll,
            crate::test_support::creature_types::synthetic_vocabulary(),
            std::sync::Arc::new(spellwake_contracts::names::NameRules::literal_names()),
        ))
        .unwrap();
        state.settle_checks(state.revision()).unwrap()
    }
    #[test]
    fn activated_cost_failures_preserve_nonzero_resources_and_no_entry_or_snapshot_escapes() {
        for cost in [SourceCost::Tap, SourceCost::Sacrifice] {
            for fault in 0..6 {
                let mut state = activation_window(cost);
                let source = state.window.cards().current_identity(CardId(1)).unwrap();
                let target =
                    TargetRef::Object(state.window.cards().current_identity(CardId(2)).unwrap());
                let error = match fault {
                    0 => {
                        state.revision = u64::MAX;
                        StackWindowError::RevisionExhausted
                    }
                    1 => {
                        state.window.set_test_revision(u64::MAX);
                        StackWindowError::RevisionExhausted
                    }
                    2 => {
                        state.window.set_test_priority_revision(u64::MAX);
                        StackWindowError::Priority(
                            crate::priority::PriorityError::RevisionExhausted,
                        )
                    }
                    3 => {
                        state.window.set_test_object_revision(u64::MAX);
                        StackWindowError::Objects(
                            crate::object_identity::ObjectIdentityError::RevisionExhausted,
                        )
                    }
                    4 => {
                        state.window.set_test_next_entry(u64::MAX);
                        StackWindowError::EntryIdentityExhausted
                    }
                    _ => {
                        state.window.set_test_next_object_identity(u64::MAX);
                        StackWindowError::Objects(
                            crate::object_identity::ObjectIdentityError::IdentityExhausted,
                        )
                    }
                };
                let before = state.clone();
                let result = state.activate_ability(
                    state.revision(),
                    SeatId(0),
                    source,
                    ActivatedAbilityId(7),
                    target,
                    None,
                );
                if fault == 5 && cost == SourceCost::Tap {
                    let accepted = result.unwrap();
                    assert_eq!(accepted.result().activation.resulting_source, source);
                    assert_eq!(accepted.state().mana(SeatId(0), ManaType::Red), 6.into());
                } else {
                    assert_eq!(result, Err(EffectError::Window(error)));
                }
                assert_eq!(state, before);
                assert_eq!(
                    state.window.cards().get(source).unwrap().tapped(),
                    Some(false)
                );
                assert!(state.payloads.is_empty());
                assert!(state.window.entries().is_empty());
                assert_eq!(state.mana(SeatId(0), ManaType::Red), 7.into());
                assert_eq!(state.mana(SeatId(1), ManaType::Blue), 11.into());
            }
        }
    }
    #[test]
    fn activated_target_capture_failure_keeps_source_and_resources() {
        for cost in [SourceCost::Tap, SourceCost::Sacrifice] {
            let state = activation_window(cost);
            let source = state.window.cards().current_identity(CardId(1)).unwrap();
            let target =
                TargetRef::Object(state.window.cards().current_identity(CardId(2)).unwrap());
            let before = state.clone();
            let result = state.window.record_paid_targeted_activation(
                state.window.revision(),
                SeatId(0),
                source,
                cost,
                vec![TargetClause {
                    id: TargetClauseId(0),
                    minimum: 1,
                    maximum: 1,
                }],
                vec![TargetChoice {
                    id: TargetId(0),
                    clause: TargetClauseId(1),
                    target,
                }],
            );
            assert!(matches!(result, Err(StackWindowError::Targets(_))));
            assert_eq!(state, before);
            assert_eq!(
                state.window.cards().get(source).unwrap().tapped(),
                Some(false)
            );
            assert!(state.payloads.is_empty());
            assert_eq!(state.mana(SeatId(0), ManaType::Red), 7.into());
            assert_eq!(state.mana(SeatId(1), ManaType::Blue), 11.into());
        }
    }
    fn color_window() -> EffectWindow {
        let a = SeatId(0);
        let b = SeatId(1);
        let ability = ManaAbility {
            id: ManaAbilityId(7),
            output: ManaOutput::ChooseColor {
                amount: 3.into(),
                colors: BTreeSet::from([ManaType::Red, ManaType::Green]),
            },
        };
        let state = EffectWindow::new(GameSeed::with_uniform_life(
            vec![a, b],
            a,
            vec![
                SeedCard {
                    card: CardId(1),
                    owner: b,
                    destination: ObjectDestination::Battlefield { controller: a },
                    program: Program::land(
                        std::collections::BTreeSet::new(),
                        vec![ability.clone()],
                    ),
                },
                SeedCard {
                    card: CardId(2),
                    owner: a,
                    destination: ObjectDestination::Battlefield { controller: b },
                    program: Program::land(std::collections::BTreeSet::new(), vec![ability]),
                },
            ],
            vec![CardId(1), CardId(2)],
            vec![
                FloatingMana {
                    player: a,
                    mana_type: ManaType::Red,
                    amount: 5.into(),
                },
                FloatingMana {
                    player: b,
                    mana_type: ManaType::Blue,
                    amount: 11.into(),
                },
            ],
            20.into(),
            CombatRules::UnrestrictedFreeForAll,
            crate::test_support::creature_types::synthetic_vocabulary(),
            std::sync::Arc::new(spellwake_contracts::names::NameRules::literal_names()),
        ))
        .unwrap();
        state.settle_checks(state.revision()).unwrap()
    }
    fn paid_color() -> (EffectWindow, ManaColorChoice) {
        let initial = color_window();
        let source = initial.window.cards().current_identity(CardId(1)).unwrap();
        let (state, ManaActivationResult::Pending { frame }, _) = initial
            .begin_mana_activation(
                initial.revision(),
                SeatId(0),
                source,
                crate::mana::ManaAbilityRef::Printed(ManaAbilityId(7)),
            )
            .unwrap()
            .into_parts()
        else {
            panic!("expected color choice")
        };
        (state, frame)
    }
    #[test]
    fn color_begin_revision_failures_preserve_unpaid_nonzero_state() {
        for fault in 0..4 {
            let mut state = color_window();
            let source = state.window.cards().current_identity(CardId(1)).unwrap();
            let error = match fault {
                0 => {
                    state.revision = u64::MAX;
                    StackWindowError::RevisionExhausted
                }
                1 => {
                    state.window.set_test_revision(u64::MAX);
                    StackWindowError::RevisionExhausted
                }
                2 => {
                    state.window.set_test_priority_revision(u64::MAX);
                    StackWindowError::Priority(crate::priority::PriorityError::RevisionExhausted)
                }
                _ => {
                    state.window.set_test_object_revision(u64::MAX);
                    StackWindowError::Objects(
                        crate::object_identity::ObjectIdentityError::RevisionExhausted,
                    )
                }
            };
            let before = state.clone();
            assert_eq!(
                state.begin_mana_activation(
                    state.revision(),
                    SeatId(0),
                    source,
                    crate::mana::ManaAbilityRef::Printed(ManaAbilityId(7))
                ),
                Err(EffectError::Window(error))
            );
            assert_eq!(state, before);
            assert_eq!(
                state.window.cards().get(source).unwrap().tapped(),
                Some(false)
            );
            assert_eq!(state.mana(SeatId(0), ManaType::Red), 5.into());
            assert_eq!(state.mana(SeatId(1), ManaType::Blue), 11.into());
        }
    }
    #[test]
    fn color_begin_and_reply_require_no_card_or_stack_identity_allocation() {
        let mut state = color_window();
        let source = state.window.cards().current_identity(CardId(1)).unwrap();
        state.window.set_test_next_entry(u64::MAX);
        state.window.set_test_next_object_identity(u64::MAX);
        let before = state.clone();
        let (paid, ManaActivationResult::Pending { frame }, _) = state
            .begin_mana_activation(
                state.revision(),
                SeatId(0),
                source,
                crate::mana::ManaAbilityRef::Printed(ManaAbilityId(7)),
            )
            .unwrap()
            .into_parts()
        else {
            panic!("expected choice")
        };
        assert_eq!(state, before);
        assert_eq!(paid.mana(SeatId(0), ManaType::Green), 0.into());
        assert_eq!(
            paid.window.cards().get(source).unwrap().tapped(),
            Some(true)
        );
        assert!(paid.window.entries().is_empty());
        let complete = paid
            .resume_mana_color(paid.revision(), &frame, SeatId(0), ManaType::Green)
            .unwrap();
        assert_eq!(complete.result().source, source);
        assert_eq!(complete.state().mana(SeatId(0), ManaType::Green), 3.into());
        assert_eq!(complete.state().mana(SeatId(1), ManaType::Blue), 11.into());
        assert!(complete.state().window.entries().is_empty());
    }
    #[test]
    fn color_token_mutations_cannot_replace_paid_work_or_credit() {
        let (state, frame) = paid_color();
        let other = state.window.cards().current_identity(CardId(2)).unwrap();
        for field in 0..6 {
            let mut changed = frame.clone();
            match field {
                0 => changed.revision += 1,
                1 => changed.source = other,
                2 => changed.ability = crate::mana::ManaAbilityRef::Printed(ManaAbilityId(8)),
                3 => changed.chooser = SeatId(1),
                4 => changed.amount = 4.into(),
                _ => changed.colors.clear(),
            }
            let before = state.clone();
            assert_eq!(
                state.resume_mana_color(state.revision(), &changed, SeatId(0), ManaType::Red),
                Err(EffectError::WrongManaColorFrame)
            );
            assert_eq!(state, before);
            assert_eq!(
                state.window.cards().get(frame.source()).unwrap().tapped(),
                Some(true)
            );
            assert_eq!(state.mana(SeatId(0), ManaType::Red), 5.into());
            assert_eq!(state.mana(SeatId(1), ManaType::Blue), 11.into());
        }
    }
    #[test]
    fn color_resume_revision_exhaustion_preserves_paid_nonzero_state() {
        let (mut state, _) = paid_color();
        let frame = state.set_test_mana_revision(u64::MAX);
        let before = state.clone();
        assert_eq!(
            state.resume_mana_color(u64::MAX, &frame, SeatId(0), ManaType::Red),
            Err(EffectError::Window(StackWindowError::RevisionExhausted))
        );
        assert_eq!(state, before);
        assert_eq!(
            state.window.cards().get(frame.source()).unwrap().tapped(),
            Some(true)
        );
        assert_eq!(state.mana(SeatId(0), ManaType::Red), 5.into());
        assert_eq!(state.mana(SeatId(1), ManaType::Blue), 11.into());
    }
    #[test]
    fn mana_activation_operational_failures_and_no_stack_allocation() {
        let a = SeatId(0);
        let b = SeatId(1);
        let initial = EffectWindow::new(GameSeed::with_uniform_life(
            vec![a, b],
            a,
            vec![SeedCard {
                card: CardId(1),
                owner: b,
                destination: ObjectDestination::Battlefield { controller: a },
                program: Program::land(
                    std::collections::BTreeSet::new(),
                    vec![ManaAbility {
                        id: ManaAbilityId(4),
                        output: ManaOutput::Fixed(BTreeMap::from([(ManaType::Green, 3.into())])),
                    }],
                ),
            }],
            vec![CardId(1)],
            vec![
                FloatingMana {
                    player: a,
                    mana_type: ManaType::Green,
                    amount: 7.into(),
                },
                FloatingMana {
                    player: b,
                    mana_type: ManaType::Blue,
                    amount: 11.into(),
                },
            ],
            20.into(),
            CombatRules::UnrestrictedFreeForAll,
            crate::test_support::creature_types::synthetic_vocabulary(),
            std::sync::Arc::new(spellwake_contracts::names::NameRules::literal_names()),
        ))
        .unwrap();
        let initial = initial.settle_checks(initial.revision()).unwrap();
        let source = initial.window.cards().current_identity(CardId(1)).unwrap();
        for fault in 0..4 {
            let mut state = initial.clone();
            let expected = match fault {
                0 => {
                    state.revision = u64::MAX;
                    EffectError::Window(StackWindowError::RevisionExhausted)
                }
                1 => {
                    state.window.set_test_revision(u64::MAX);
                    EffectError::Window(StackWindowError::RevisionExhausted)
                }
                2 => {
                    state.window.set_test_object_revision(u64::MAX);
                    EffectError::Window(StackWindowError::Objects(
                        crate::object_identity::ObjectIdentityError::RevisionExhausted,
                    ))
                }
                _ => {
                    state.window.set_test_priority_revision(u64::MAX);
                    EffectError::Window(StackWindowError::Priority(
                        crate::priority::PriorityError::RevisionExhausted,
                    ))
                }
            };
            let before = state.clone();
            assert_eq!(
                state.activate_mana(
                    state.revision(),
                    a,
                    source,
                    crate::mana::ManaAbilityRef::Printed(ManaAbilityId(4))
                ),
                Err(expected)
            );
            assert_eq!(state, before);
            assert_eq!(state.mana(a, ManaType::Green), 7.into());
            assert_eq!(state.mana(b, ManaType::Blue), 11.into());
            assert_eq!(
                state.window.cards().get(source).unwrap().tapped(),
                Some(false)
            );
        }
        let mut state = initial;
        state.window.set_test_next_entry(u64::MAX);
        let activated = state
            .activate_mana(
                state.revision(),
                a,
                source,
                crate::mana::ManaAbilityRef::Printed(ManaAbilityId(4)),
            )
            .unwrap();
        assert_eq!(activated.state().mana(a, ManaType::Green), 10.into());
        assert_eq!(
            activated
                .state()
                .window
                .cards()
                .get(source)
                .unwrap()
                .tapped(),
            Some(true)
        );
        assert_eq!(activated.result().source, source);
        assert!(activated.state().window.entries().is_empty());
        assert_eq!(state.mana(a, ManaType::Green), 7.into());
    }
    #[test]
    fn paid_cast_operational_failures_preserve_nonzero_pools_and_source() {
        let a = SeatId(0);
        let b = SeatId(1);
        let mut state = EffectWindow::new(GameSeed::with_uniform_life(
            vec![a, b],
            a,
            vec![
                SeedCard {
                    card: CardId(1),
                    owner: a,
                    destination: ObjectDestination::Hand,
                    program: Program::instant(
                        ManaCost::one(ManaType::Red),
                        ExecutableBody::new(
                            BodyTarget::SingleCreature,
                            vec![Instruction::Damage { amount: 1.into() }],
                        ),
                    ),
                },
                SeedCard {
                    card: CardId(2),
                    owner: b,
                    destination: ObjectDestination::Battlefield { controller: b },
                    program: Program::creature(
                        None,
                        1.into(),
                        10.into(),
                        std::collections::BTreeSet::new(),
                    ),
                },
            ],
            vec![CardId(2)],
            vec![
                FloatingMana {
                    player: a,
                    mana_type: ManaType::Red,
                    amount: 2.into(),
                },
                FloatingMana {
                    player: b,
                    mana_type: ManaType::Red,
                    amount: 99.into(),
                },
            ],
            20.into(),
            CombatRules::UnrestrictedFreeForAll,
            crate::test_support::creature_types::synthetic_vocabulary(),
            std::sync::Arc::new(spellwake_contracts::names::NameRules::literal_names()),
        ))
        .unwrap();
        state = state.settle_checks(state.revision()).unwrap();
        let spell = state.window.cards().current_identity(CardId(1)).unwrap();
        let target = TargetRef::Object(state.window.cards().current_identity(CardId(2)).unwrap());
        state.revision = u64::MAX;
        let saved = state.clone();
        assert_eq!(
            state.cast_with_payment(state.revision(), a, spell, target, None),
            Err(EffectError::Window(StackWindowError::RevisionExhausted))
        );
        assert_eq!(state, saved);
        state.revision = 1;
        state.window.set_test_revision(u64::MAX);
        let saved = state.clone();
        // Exact payment validates before the child window fails to allocate its successor.
        assert_eq!(
            state.cast_with_payment(state.revision(), a, spell, target, None),
            Err(EffectError::Window(StackWindowError::RevisionExhausted))
        );
        assert_eq!(state, saved);
        assert_eq!(state.mana(a, ManaType::Red), 2.into());
        assert_eq!(state.mana(b, ManaType::Red), 99.into());
        state.window.set_test_revision(1);
        state.window.set_test_next_entry(u64::MAX);
        let saved = state.clone();
        assert_eq!(
            state.cast_with_payment(state.revision(), a, spell, target, None),
            Err(EffectError::Window(
                StackWindowError::EntryIdentityExhausted
            ))
        );
        assert_eq!(state, saved);
        assert_eq!(state.mana(a, ManaType::Red), 2.into());
        assert_eq!(
            state.window.cards().get(spell).unwrap().zone(),
            ObjectZone::Hand(a)
        );
    }
    #[test]
    fn instruction_index_alone_cannot_redirect_a_copy_continuation() {
        use crate::game::{GameCommand, GameContext, GameId, GameInput, GameState};
        let a = SeatId(0);
        let b = SeatId(1);
        let instant = |card, owner, instructions, body_target| SeedCard {
            card: CardId(card),
            owner,
            destination: ObjectDestination::Hand,
            program: Program::instant(
                ManaCost::one(ManaType::Blue),
                ExecutableBody::new(body_target, instructions),
            ),
        };
        let mut game = GameState::initialize(
            GameId(30),
            GameContext::new(
                "test".into(),
                "qualification-synthetic".into(),
                "cards".into(),
                "programs".into(),
                "profile".into(),
                crate::test_support::creature_types::synthetic_vocabulary().id(),
                spellwake_contracts::names::NameRules::literal_names().id(),
            )
            .unwrap(),
            GameSeed::with_uniform_life(
                vec![a, b],
                a,
                vec![
                    instant(
                        1,
                        a,
                        vec![Instruction::Damage { amount: 1.into() }],
                        BodyTarget::SingleCreature,
                    ),
                    instant(
                        2,
                        b,
                        vec![Instruction::CopySpell, Instruction::CopySpell],
                        BodyTarget::SingleSpell(SpellTargetFilter::CopyableInstructionSpell),
                    ),
                    SeedCard {
                        card: CardId(3),
                        owner: b,
                        destination: ObjectDestination::Battlefield { controller: b },
                        program: Program::creature(
                            None,
                            1.into(),
                            10.into(),
                            std::collections::BTreeSet::new(),
                        ),
                    },
                ],
                vec![CardId(3)],
                vec![
                    FloatingMana {
                        player: a,
                        mana_type: ManaType::Blue,
                        amount: 1.into(),
                    },
                    FloatingMana {
                        player: b,
                        mana_type: ManaType::Blue,
                        amount: 1.into(),
                    },
                ],
                20.into(),
                CombatRules::UnrestrictedFreeForAll,
                crate::test_support::creature_types::synthetic_vocabulary(),
                std::sync::Arc::new(spellwake_contracts::names::NameRules::literal_names()),
            ),
        )
        .unwrap()
        .into_state();
        let target = game
            .execution()
            .window()
            .cards()
            .current_identity(CardId(3))
            .unwrap();
        let spell = game
            .execution()
            .window()
            .cards()
            .current_identity(CardId(1))
            .unwrap();
        for (actor, command) in [
            (
                a,
                GameCommand::Cast {
                    spell,
                    target: TargetRef::Object(target),
                },
            ),
            (a, GameCommand::Pass),
        ] {
            game = game
                .advance(GameInput {
                    basis: game.basis().clone(),
                    actor,
                    command,
                })
                .unwrap()
                .into_state();
        }
        let original = game.execution().window().entries()[0].id();
        let spell = game
            .execution()
            .window()
            .cards()
            .current_identity(CardId(2))
            .unwrap();
        for (actor, command) in [
            (
                b,
                GameCommand::Cast {
                    spell,
                    target: TargetRef::Stack(original),
                },
            ),
            (b, GameCommand::Pass),
            (a, GameCommand::Pass),
        ] {
            game = game
                .advance(GameInput {
                    basis: game.basis().clone(),
                    actor,
                    command,
                })
                .unwrap()
                .into_state();
        }
        let crate::game::RuleFrame::CopyTargets(frame) = game.pending_decision().unwrap().frame()
        else {
            panic!("copy choice required");
        };
        let mut wrong = frame.clone();
        wrong.instruction = 1;
        let state = game.execution();
        let saved = state.clone();
        assert_eq!(
            state.resume_rule(
                state.revision(),
                &EffectDecision::CopyTargets(wrong),
                b,
                DecisionReply::CopyTargets(CopyTargetChoice::Retain),
                None,
            ),
            Err(EffectError::WrongResolutionFrame)
        );
        assert_eq!(state, &saved);
        assert_eq!(frame.instruction_index(), 0);
    }
    #[test]
    fn executor_revision_exhaustion_is_checked_without_mutating_child_or_resources() {
        let a = SeatId(0);
        let mut state = EffectWindow::new(GameSeed::with_uniform_life(
            vec![a, SeatId(1)],
            a,
            vec![],
            vec![],
            vec![],
            20.into(),
            CombatRules::UnrestrictedFreeForAll,
            crate::test_support::creature_types::synthetic_vocabulary(),
            std::sync::Arc::new(spellwake_contracts::names::NameRules::literal_names()),
        ))
        .unwrap();
        state = state.settle_checks(state.revision()).unwrap();
        state.revision = u64::MAX;
        let saved = state.clone();
        assert_eq!(
            state.pass(u64::MAX, a),
            Err(EffectError::Window(StackWindowError::RevisionExhausted))
        );
        assert_eq!(state, saved);
        assert_eq!(
            state.pass(0, a),
            Err(EffectError::Window(StackWindowError::StaleRevision))
        );
        assert_eq!(state, saved);
    }
}

#[cfg(test)]
mod setup_life_tests {
    use super::*;
    use crate::object_identity::ObjectIdentityError;

    #[test]
    fn late_creature_check_failure_does_not_publish_collected_life_losses() {
        let a = SeatId(0);
        let mut state = EffectWindow::new(GameSeed::with_uniform_life(
            vec![a, SeatId(1)],
            a,
            vec![SeedCard {
                card: CardId(901),
                owner: a,
                destination: ObjectDestination::Battlefield { controller: a },
                program: Program::creature(
                    None,
                    1.into(),
                    0.into(),
                    std::collections::BTreeSet::new(),
                ),
            }],
            vec![CardId(901)],
            vec![],
            0.into(),
            CombatRules::UnrestrictedFreeForAll,
            crate::test_support::creature_types::synthetic_vocabulary(),
            std::sync::Arc::new(spellwake_contracts::names::NameRules::literal_names()),
        ))
        .unwrap();
        for identity in [false, true] {
            if identity {
                state.window.set_test_next_object_identity(u64::MAX);
            } else {
                state.window.set_test_object_revision(u64::MAX);
            }
            let before = state.clone();
            assert_eq!(
                state.settle_checks(state.revision()),
                Err(EffectError::Window(StackWindowError::Objects(
                    if identity {
                        ObjectIdentityError::IdentityExhausted
                    } else {
                        ObjectIdentityError::RevisionExhausted
                    }
                )))
            );
            assert_eq!(state, before);
            assert!(state.state_based_losses().is_empty());
            state.window.set_test_object_revision(1);
        }
    }
}
