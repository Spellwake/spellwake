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
mod mutate;
pub use mutate::MutationFrame;
mod myriad;
pub use myriad::MyriadDefenderFrame;
pub(crate) mod cast_copies;
pub use cast_copies::CapturedAbilityCopy;
mod attachments;
mod casting_resources;
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
mod day_night;
mod emblems;
mod end_step_cast;
mod impulse;
mod library_rewrite;
mod ninja;
mod speed;
mod splice;
mod votes;
pub(crate) use combat_flow::AttackAllocationCut;
mod ability_activation;
mod apply_instruction;
mod ascend;
mod casting;
mod commanders;
mod control_flow;
mod creature_type_flow;
mod damage_flow;
mod entry_copy;
mod entry_counters;
mod extra_costs;
mod mana_activation;
mod mana_domains;
mod mana_provenance;
mod mana_sacrifice;
mod monarch;
mod pass_c_ops;
mod progress_rules;
mod rule_resume;
pub use commanders::CommanderRecord;
mod draw_replacement;
pub(crate) mod event_replacement;
mod face_down;
mod library_access;
mod library_search;
mod library_top;
mod look_top;
mod meld;
mod soulbond;
pub use draw_replacement::{DrawReplacementChoice, DrawReplacementDomain, DrawReplacementFrame};
pub use library_access::LibraryTopAccess;
mod revealed_piles;
pub use revealed_piles::{PileChoice, PileChoiceFrame};
mod cascade;
mod life_flow;
mod plot;
mod suspend;
pub(crate) use plot::ExileHandAction;
use suspend::ResolvingCast;
pub use suspend::{
    ExileCastAbility, ExileCastMode, ExileCastModes, ExileCastOffer, ExilePermanentTargets,
};
/// Original public name for a suspend spell offer.
pub type SuspendedCastOffer = ExileCastOffer;
mod counter_keywords;
mod counter_order;
mod delayed_graveyard;
mod goad;
mod growth;
mod permanent_counters;
mod power_abilities;
mod power_up;
mod source_boost;
pub use counter_order::CounterReplacementChoice;
mod amass;
mod counter_removal;
mod proliferate;
pub use counter_removal::CounterRemovalChoice;
pub use proliferate::{CounterKind, CustomCounterKey, ProliferateChoice};
mod activation_cost;
mod graveyard_exile;
mod graveyard_shuffle;
mod graveyard_source;
mod protection;
mod regeneration;
mod selected_activation_cost;
mod transfers;
mod unearth;
mod upkeep;
mod ward;
use library_top::LibraryTopWork;
pub use library_top::{
    LibraryAction, LibraryActionKind, LibraryChoiceKind, LibraryPublicMove, LibraryTopFrame,
    LibraryTopReply,
};
mod lifecycle_flow;
mod modes;
#[cfg(test)]
mod names_snapshot_tests;
mod occurrence_flow;
mod opening_capture;
mod player_departure;
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
    ResolutionSelectionFrame, ResolutionSelectionKind, ResolutionSelectionOperation,
    ResolutionSelectionRequirement, ResolutionSelectionStage, ResolutionSelectionState,
};
pub use zone_replacement::*;
#[cfg(test)]
pub(crate) mod private_qualification;
#[cfg(test)]
mod supertype_projection_tests;
mod token_entry_flow;
mod token_offers;
#[cfg(test)]
mod transport_tests;
#[cfg(test)]
mod world_history_tests;
use crate::activated::{
    AbilityActivationRecord, AbilityActivationTransition, ActivatedAbilityId, ActivationRequest,
};
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
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::sync::Arc;
use token_entry_flow::TokenEntryWork;

pub use crate::program::{
    BodyTarget, ExecutableBody, Instruction, Program, ProgramKind, SpellFilter,
};
use crate::program::{
    CardType, CreatureTypeSlot, EventValueKind, ObjectFilter, PlayerStatExpr, ValueExpr,
};
/// One trusted resolution-time choice: retaining is distinct from fresh capture.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CopyTargetChoice {
    Retain,
    Change(TargetRef),
}
/// Owned post-target-check continuation. Only the executor constructs this frame.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CopyResolution {
    revision: u64,
    entry: StackEntryId,
    original: StackEntryId,
    target_ordinal: usize,
    chooser: SeatId,
    instruction: usize,
}
impl CopyResolution {
    /// Zero-based target occurrence currently being retained or changed.
    pub fn target_ordinal(&self) -> usize {
        self.target_ordinal
    }

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
    revision: u64,
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
    target: DamageRecipient,
    amount: RulesInteger,
}
impl DamageAllocationDomain {
    /// Actual recipient of this eligible damage component.
    pub fn target(&self) -> DamageRecipient {
        self.target
    }

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
    revision: u64,
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
/// Controller's frozen pre-entry choice for one exact entering incarnation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntryChoiceFrame {
    revision: u64,
    chooser: SeatId,
    object: ObjectId,
    rule: crate::land::EntryTap,
    eligible_reveals: Vec<ObjectId>,
    card_names: Option<Arc<crate::land::NonlandCardNames>>,
}
/// Relative timestamp order for one player's simultaneous linked returns.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinkedReturnOrderFrame {
    revision: u64,
    chooser: SeatId,
    objects: Vec<ObjectId>,
    destination: crate::program::PublicZoneDestination,
}
/// Legal existing hosts for an Aura returning from linked exile without being cast.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinkedAuraChoiceFrame {
    revision: u64,
    chooser: SeatId,
    object: ObjectId,
    domain: Vec<TargetRef>,
}
impl LinkedAuraChoiceFrame {
    /// Executor revision at which this choice was offered.
    pub fn revision(&self) -> u64 {
        self.revision
    }
    /// The controller of the entering Aura.
    pub fn chooser(&self) -> SeatId {
        self.chooser
    }
    /// The exact pre-entry Aura incarnation.
    pub fn object(&self) -> ObjectId {
        self.object
    }
    /// Existing legal hosts; choosing one does not target it.
    pub fn domain(&self) -> &[TargetRef] {
        &self.domain
    }
}
impl LinkedReturnOrderFrame {
    /// Executor revision at which this order was offered.
    pub fn revision(&self) -> u64 {
        self.revision
    }
    /// Controller assigning the relative timestamps.
    pub fn chooser(&self) -> SeatId {
        self.chooser
    }
    /// Exact exile incarnations entering under that controller.
    pub fn objects(&self) -> &[ObjectId] {
        &self.objects
    }
    /// Battlefield timestamp order or graveyard top-to-bottom order.
    pub fn destination(&self) -> crate::program::PublicZoneDestination {
        self.destination
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LinkedReturnOrigin {
    Exile,
    ExileWithCounters {
        entry: StackEntryId,
        instruction: usize,
    },
    ChosenExile {
        controller: Option<SeatId>,
        entry: StackEntryId,
        instruction: usize,
        tapped: bool,
    },
    Sacrifice {
        zone: ObjectZone,
        controller: SeatId,
        entry: StackEntryId,
        instruction: usize,
    },
    Manifest {
        player: SeatId,
        entry: StackEntryId,
        instruction: usize,
    },
    Hand {
        player: SeatId,
        entry: StackEntryId,
        instruction: usize,
        tapped: bool,
    },
    HandOrGraveyardAuras {
        player: SeatId,
        host: ObjectId,
        entry: StackEntryId,
        instruction: usize,
    },
    GraveyardTransformed {
        player: SeatId,
        controller: SeatId,
        entry: StackEntryId,
        instruction: usize,
    },
    Graveyard {
        player: Option<SeatId>,
        controller: Option<SeatId>,
        subtype_counters: Option<(CreatureTypeId, u16)>,
        entry_additions: crate::program::EntryAdditions,
        entry: StackEntryId,
        instruction: usize,
        tapped: bool,
        forest: bool,
        corpse: bool,
    },
    LibraryTop {
        player: SeatId,
        entry: StackEntryId,
        instruction: usize,
        tapped: bool,
    },
}
impl LinkedReturnOrigin {
    fn entry_controller(self, card: &GameObject) -> SeatId {
        match self {
            Self::ChosenExile {
                controller: Some(controller),
                ..
            }
            | Self::Graveyard {
                controller: Some(controller),
                ..
            }
            | Self::Sacrifice { controller, .. }
            | Self::GraveyardTransformed { controller, .. } => controller,
            _ => card.owner(),
        }
    }
    fn accepts(self, card: &GameObject) -> bool {
        card.card_id().is_some()
            && match self {
                Self::Exile | Self::ExileWithCounters { .. } | Self::ChosenExile { .. } => {
                    card.zone() == ObjectZone::Exile
                }
                Self::Sacrifice { zone, .. } => card.zone() == zone,
                Self::Hand { player, .. } => card.zone() == ObjectZone::Hand(player),
                Self::HandOrGraveyardAuras { player, .. } => {
                    matches!(card.zone(), ObjectZone::Hand(owner) | ObjectZone::Graveyard(owner) if owner == player)
                }
                Self::Graveyard { player, .. } => {
                    matches!(card.zone(), ObjectZone::Graveyard(owner) if player.is_none_or(|player| player == owner))
                }
                Self::GraveyardTransformed { player, .. } => {
                    card.zone() == ObjectZone::Graveyard(player)
                }
                Self::Manifest { player, .. } | Self::LibraryTop { player, .. } => {
                    card.zone() == ObjectZone::Library(player)
                }
            }
    }
    fn face_down_entry(self) -> Option<crate::face_down::FaceDownKind> {
        match self {
            Self::Manifest { .. } => Some(crate::face_down::FaceDownKind::Manifest),
            Self::Graveyard { forest: true, .. } => Some(crate::face_down::FaceDownKind::Forest),
            _ => None,
        }
    }
    fn reason(self, from: ObjectZone) -> crate::occurrence::ZoneReason {
        match self {
            Self::Sacrifice { .. } => crate::occurrence::ZoneReason::ReturnFromSacrifice,
            Self::Manifest { .. } => crate::occurrence::ZoneReason::Manifest,
            Self::ExileWithCounters { .. } | Self::ChosenExile { .. } => {
                crate::occurrence::ZoneReason::ExileToBattlefield
            }
            Self::Exile => crate::occurrence::ZoneReason::ReturnFromLinkedExile,
            Self::Hand { .. } => crate::occurrence::ZoneReason::HandToBattlefield,
            Self::HandOrGraveyardAuras { .. } => match from {
                ObjectZone::Graveyard(_) => {
                    crate::occurrence::ZoneReason::ReturnFromGraveyardToBattlefield
                }
                _ => crate::occurrence::ZoneReason::HandToBattlefield,
            },
            Self::Graveyard { .. } | Self::GraveyardTransformed { .. } => {
                crate::occurrence::ZoneReason::ReturnFromGraveyardToBattlefield
            }
            Self::LibraryTop { .. } => crate::occurrence::ZoneReason::LookTopToBattlefield,
        }
    }
    fn cause(self) -> crate::occurrence::EventCause {
        match self {
            Self::Exile => crate::occurrence::EventCause::MechanismUnit,
            Self::ExileWithCounters { entry, instruction }
            | Self::Sacrifice {
                entry, instruction, ..
            }
            | Self::Manifest {
                entry, instruction, ..
            }
            | Self::ChosenExile {
                entry, instruction, ..
            }
            | Self::Hand {
                entry, instruction, ..
            }
            | Self::HandOrGraveyardAuras {
                entry, instruction, ..
            }
            | Self::Graveyard {
                entry, instruction, ..
            }
            | Self::GraveyardTransformed {
                entry, instruction, ..
            }
            | Self::LibraryTop {
                entry, instruction, ..
            } => crate::occurrence::EventCause::Instruction { entry, instruction },
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
struct LinkedReturnWork {
    library_bottom: Option<Vec<ObjectId>>,
    origin: LinkedReturnOrigin,
    wake_entry: Option<StackEntryId>,
    objects: Vec<ObjectId>,
    orders: BTreeMap<SeatId, Vec<ObjectId>>,
    graveyard_orders: BTreeMap<SeatId, Vec<ObjectId>>,
    choices: BTreeMap<ObjectId, crate::land::EntryChoice>,
    auras: BTreeMap<ObjectId, TargetRef>,
    replacements: Option<ZoneReplacementBatch>,
}
#[derive(Debug, Clone, PartialEq, Eq)]
struct DeferredActivation {
    activation: crate::activated::AbilityActivationRecord,
    receipt: ManaPaymentRecord,
}
#[derive(Debug, Clone, PartialEq, Eq)]
struct DeferredCast {
    library_tops: BTreeMap<SeatId, ObjectId>,
    resources: Vec<crate::mana::CastResource>,
    entry: StackEntryId,
    actor: SeatId,
    receipt: ManaPaymentRecord,
    mana_value: RulesInteger,
    copy_source_needed: bool,
}
#[derive(Debug, Clone, PartialEq, Eq)]
struct CastCostReplacementWork {
    batch: ZoneReplacementBatch,
    objects: Vec<ObjectId>,
    graveyard_orders: BTreeMap<SeatId, Vec<ObjectId>>,
}
/// Owned post-announcement data needed to finish a child cast after its mana window.
#[derive(Debug, Clone, PartialEq, Eq)]
struct PreparedCastPayment {
    library_tops: BTreeMap<SeatId, ObjectId>,
    entry: StackEntryId,
    actor: SeatId,
    cost: ManaCost,
    unpaid_mana: ManaCost,
    payment: crate::mana::CastingPayment,
    life_payment: Option<RulesInteger>,
    mana_value: RulesInteger,
    sunburst: bool,
    copy_source_needed: bool,
    original_spell: GameObject,
    graveyard_below: Vec<ObjectId>,
    announcement_event: crate::occurrence::EventId,
    foretold_at: Option<u64>,
    plotted_at: Option<u64>,
    warped_at: Option<u64>,
    time_counters: Option<RulesInteger>,
}
/// A child cast interrupted while one of its mana abilities is dealing damage.
#[derive(Debug, Clone, PartialEq, Eq)]
struct DeferredManaCast {
    prepared: PreparedCastPayment,
    remaining: Vec<crate::mana::CastManaActivation>,
    parent_entry: StackEntryId,
    kind: ExileCastAbility,
    turn: Option<TurnScheduler>,
    parent_cursor: Option<ResolutionCursor>,
    damage_journal: DamageJournal,
}
/// A child cast whose life payment is temporarily impossible after mana damage.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CastRecoveryFrame {
    revision: u64,
    chooser: SeatId,
    spell: ObjectId,
    life_cost: RulesInteger,
    mana_cost: ManaCost,
}
impl CastRecoveryFrame {
    pub fn revision(&self) -> u64 {
        self.revision
    }
    pub fn chooser(&self) -> SeatId {
        self.chooser
    }
    pub fn spell(&self) -> ObjectId {
        self.spell
    }
    pub fn life_cost(&self) -> &RulesInteger {
        &self.life_cost
    }
    pub fn mana_cost(&self) -> &ManaCost {
        &self.mana_cost
    }
}
/// Retry the remaining payment, or reverse this unfinished child cast.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CastRecoveryChoice {
    Retry(Option<ManaPayment>),
    RetryRestricted {
        payment: Option<ManaPayment>,
        restricted_mana: Vec<crate::mana::RestrictedManaSpend>,
    },
    Abandon,
}
impl EntryChoiceFrame {
    pub fn card_names(&self) -> &[spellwake_contracts::names::EnglishCardName] {
        self.card_names
            .as_deref()
            .map_or(&[], crate::land::NonlandCardNames::options)
    }

    /// Executor revision that issued this frame.
    pub fn revision(&self) -> u64 {
        self.revision
    }
    /// Player making the entry decision.
    pub fn chooser(&self) -> SeatId {
        self.chooser
    }
    /// Exact object before its change of zone.
    pub fn object(&self) -> ObjectId {
        self.object
    }
    /// Entry rule defining the permissible choice.
    pub fn rule(&self) -> crate::land::EntryTap {
        self.rule
    }
    /// Private eligible hand cards for an optional reveal.
    pub fn eligible_reveals(&self) -> &[ObjectId] {
        &self.eligible_reveals
    }
}
/// General N-way option choice issued during resolution (mana bundles, disjunctive choices, modes).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OptionIndexFrame {
    pub(crate) revision: u64,
    pub(crate) entry: StackEntryId,
    pub(crate) instruction: usize,
    pub(crate) chooser: SeatId,
    pub(crate) count: u8,
    pub(crate) labels: Vec<String>,
}
impl OptionIndexFrame {
    /// Revision at which this choice frame was issued.
    #[must_use]
    pub fn revision(&self) -> u64 {
        self.revision
    }
    /// Player who must choose an option index in `0..count`.
    #[must_use]
    pub fn chooser(&self) -> SeatId {
        self.chooser
    }
    /// Number of available options (`0..count`).
    #[must_use]
    pub fn count(&self) -> u8 {
        self.count
    }
    /// Human-readable labels for each offered option (`0..count`).
    #[must_use]
    pub fn labels(&self) -> &[String] {
        &self.labels
    }
}

/// Active player's CR 502.3 choice of tapped permanents with "you may choose not to untap"
/// to keep tapped during their untap step.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UntapKeepTappedFrame {
    pub(crate) revision: u64,
    pub(crate) chooser: SeatId,
    pub(crate) objects: Vec<ObjectId>,
}

impl UntapKeepTappedFrame {
    pub fn revision(&self) -> u64 {
        self.revision
    }
    pub fn chooser(&self) -> SeatId {
        self.chooser
    }
    pub fn objects(&self) -> &[ObjectId] {
        &self.objects
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EffectDecision {
    Mutation(MutationFrame),
    ManaColor(ManaColorChoice),
    ResolutionManaColor(ResolutionManaColorChoice),
    OptionIndex(OptionIndexFrame),
    RevealedPiles(PileChoiceFrame),
    MyriadDefender(MyriadDefenderFrame),
    ReselectAttackDefender(AttackDefenderFrame),
    PlaneswalkerType(PlaneswalkerTypeFrame),
    AttackDirection(AttackDirectionFrame),
    OpponentChoice(OpponentChoiceFrame),
    DamageRecipientChoice(DamageRecipientChoiceFrame),
    DrawReplacement(DrawReplacementFrame),
    LoyaltyRemoval(LoyaltyRemovalChoice),
    CastRecovery(CastRecoveryFrame),
    LinkedAuraChoice(LinkedAuraChoiceFrame),
    LinkedReturnOrder(LinkedReturnOrderFrame),
    EntryChoice(EntryChoiceFrame),
    TokenEntryChoice(crate::token::TokenEntryChoiceFrame),
    TokenAuraChoice(crate::token::TokenAuraChoiceFrame),
    /// Exact precommit selected-effect replacement request.
    ZoneReplacement(ZoneReplacementFrame),
    LibraryTop(LibraryTopFrame),
    /// Executor-issued exact affected-player selection, potentially hidden.
    ResolutionSelect(ResolutionSelectionFrame),
    /// Executor-issued exact physical owner permutation, after every selection.
    ResolutionGraveyardOrder(ResolutionGraveyardOrderFrame),
    OptionalEffect(OptionalEffectFrame),
    ReplacementChooser(ReplacementChooserFrame),
    ResolutionPayment(ResolutionPaymentFrame),
    TokenDefender(crate::token::TokenDefenderFrame),
    TimestampOrder(TokenTimestampOrder),
    TransformOrder(crate::day_night::TransformOrder),
    UntapKeepTapped(UntapKeepTappedFrame),
    Checks(CheckDecision),
    Triggers(TriggerOrder),
    TriggerTarget(TriggerTargetChoice),
    CopyTargets(CopyResolution),
    Damage(DamageDecision),
    Combat(CombatDecision),
    Proliferate(ProliferateChoice),
    CounterRemoval(CounterRemovalChoice),
    CreatureType(CreatureTypeChoice),
}
impl EffectDecision {
    pub fn chooser(&self) -> SeatId {
        match self {
            Self::ManaColor(f) => f.chooser(),
            Self::ResolutionManaColor(f) => f.chooser,
            Self::OptionIndex(f) => f.chooser,
            Self::Mutation(f) => f.chooser(),
            Self::RevealedPiles(f) => f.chooser,
            Self::MyriadDefender(f) => f.chooser(),
            Self::ReselectAttackDefender(f) => f.chooser(),
            Self::PlaneswalkerType(f) => f.chooser,
            Self::AttackDirection(f) => f.chooser,
            Self::OpponentChoice(f) => f.chooser,
            Self::DamageRecipientChoice(f) => f.chooser,
            Self::DrawReplacement(f) => f.chooser,
            Self::LoyaltyRemoval(f) => f.chooser,
            Self::CastRecovery(f) => f.chooser(),
            Self::LinkedAuraChoice(f) => f.chooser(),
            Self::LinkedReturnOrder(f) => f.chooser(),
            Self::EntryChoice(f) => f.chooser(),
            Self::TokenEntryChoice(f) => f.chooser(),
            Self::TokenAuraChoice(f) => f.chooser(),
            Self::ZoneReplacement(f) => f.chooser(),
            Self::LibraryTop(f) => f.chooser(),
            Self::ResolutionSelect(f) => f.chooser(),
            Self::ResolutionGraveyardOrder(f) => f.chooser(),
            Self::OptionalEffect(f) => f.chooser(),
            Self::ReplacementChooser(f) => f.chooser(),
            Self::ResolutionPayment(f) => f.chooser(),
            Self::TokenDefender(f) => f.chooser(),
            Self::TimestampOrder(f) => f.chooser(),
            Self::TransformOrder(f) => f.chooser(),
            Self::UntapKeepTapped(f) => f.chooser(),
            Self::Checks(f) => f.chooser(),
            Self::Triggers(f) => f.chooser(),
            Self::TriggerTarget(f) => f.chooser(),
            Self::CopyTargets(f) => f.chooser(),
            Self::Damage(f) => f.chooser(),
            Self::Combat(f) => f.chooser(),
            Self::Proliferate(f) => f.chooser(),
            Self::CounterRemoval(f) => f.chooser(),
            Self::CreatureType(f) => f.chooser(),
        }
    }
    fn required_choice_error(&self) -> EffectError {
        match self {
            Self::ManaColor(_)
            | Self::ResolutionManaColor(_)
            | Self::OptionIndex(_)
            | Self::DrawReplacement(_)
            | Self::Mutation(_)
            | Self::RevealedPiles(_)
            | Self::MyriadDefender(_)
            | Self::ReselectAttackDefender(_)
            | Self::PlaneswalkerType(_)
            | Self::AttackDirection(_)
            | Self::OpponentChoice(_)
            | Self::UntapKeepTapped(_)
            | Self::LoyaltyRemoval(_) => EffectError::SelectionRequired,
            Self::DamageRecipientChoice(_) | Self::LinkedAuraChoice(_) => {
                EffectError::SelectionRequired
            }
            Self::LinkedReturnOrder(_) => EffectError::TimestampOrderRequired,
            Self::EntryChoice(_) => EffectError::SelectionRequired,
            Self::TokenEntryChoice(_) | Self::TokenAuraChoice(_) => EffectError::SelectionRequired,
            Self::ZoneReplacement(_) => EffectError::SelectionRequired,
            Self::LibraryTop(_) => EffectError::LibraryTopChoiceRequired,
            Self::ResolutionSelect(_) | Self::ResolutionGraveyardOrder(_) => {
                EffectError::SelectionRequired
            }
            Self::OptionalEffect(_) | Self::ReplacementChooser(_) => {
                EffectError::OptionalEffectChoiceRequired
            }
            Self::ResolutionPayment(_) => EffectError::ResolutionPaymentChoiceRequired,
            Self::TokenDefender(_) => EffectError::TokenDefenderRequired,
            Self::TimestampOrder(_) | Self::TransformOrder(_) => {
                EffectError::TimestampOrderRequired
            }
            Self::CopyTargets(_) => EffectError::CopyChoiceRequired,
            Self::TriggerTarget(_) => EffectError::TriggerTargetChoiceRequired,
            Self::Proliferate(_) | Self::CounterRemoval(_) => EffectError::SelectionRequired,
            Self::CreatureType(_) => EffectError::CreatureTypeChoiceRequired,
            Self::CastRecovery(_) => EffectError::SelectionRequired,
            Self::Checks(_) | Self::Triggers(_) | Self::Damage(_) | Self::Combat(_) => {
                EffectError::DamageChoiceRequired
            }
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DecisionReply {
    /// Put the mutating spell above its target if true, below it otherwise.
    MutateOnTop(bool),
    CastRecovery(CastRecoveryChoice),
    LinkedAuraChoice(TargetRef),
    LinkedReturnOrder(Vec<ObjectId>),
    EntryChoice(crate::land::EntryChoice),
    TokenEntryChoice(crate::land::EntryChoice),
    TokenAuraChoice(TargetRef),
    /// One answer to the current precommit replacement request.
    ZoneReplacement(ZoneReplacementReply),
    LibraryTop(LibraryTopReply),
    /// Distinct members of the current executor-issued frozen selection domain.
    SelectResolution(Vec<ObjectId>),
    /// Complete top-to-bottom permutation of the current physical owner domain.
    OrderResolutionGraveyard(Vec<ObjectId>),
    OptionalEffect(OptionalEffectChoice),
    /// Pick the current participant who will answer a departed player's choice.
    ReplacementChooser(SeatId),
    CascadeCast {
        target: Option<TargetRef>,
        payment: crate::mana::CastingPayment,
    },
    ImpulseCast {
        target: Option<TargetRef>,
        payment: crate::mana::CastingPayment,
        mana_abilities: Vec<crate::mana::CastManaActivation>,
    },
    MadnessCast {
        target: Option<TargetRef>,
        payment: crate::mana::CastingPayment,
        mana_abilities: Vec<crate::mana::CastManaActivation>,
    },
    ReboundCast {
        target: Option<TargetRef>,
        payment: crate::mana::CastingPayment,
    },
    SuspendCast {
        target: Option<TargetRef>,
        payment: crate::mana::CastingPayment,
    },
    ResolutionPayment(ResolutionPaymentReply),
    TokenDefender(TargetRef),
    MyriadDefender(Option<TargetRef>),
    ReselectAttackDefender(Option<TargetRef>),
    PlaneswalkerType(crate::program::PlaneswalkerSubtype),
    /// Subset of eligible tapped permanents kept tapped during untap (CR 502.3).
    UntapKeepTapped(Vec<ObjectId>),
    /// Subset of eligible opening-hand cards put onto the battlefield pregame (CR 103.6).
    OpeningBattlefield(Vec<ObjectId>),
    AttackDirection(AttackDirection),
    OpponentChoice(SeatId),
    ChooseDamageRecipient(TargetRef),
    DrawReplacement(DrawReplacementChoice),
    ChoosePile(u8),
    OptionIndex(u8),
    ChooseLandKind(bool),
    TransformOrder(Vec<ObjectId>),
    TimestampOrder(Vec<TokenBatchMember>),
    Discard(Vec<ObjectId>),
    OrderGraveyard(Vec<ObjectId>),
    /// CR 903.9a. An empty selection leaves all commanders in their current zones.
    ReturnCommanders(Vec<ObjectId>),
    /// Select one offered conflict and one exact member of its frozen domain.
    KeepLegend {
        conflict: crate::checks::LegendConflictId,
        keep: ObjectId,
    },
    OrderTriggers(Vec<TriggerInstanceId>),
    TriggerTarget(TargetRef),
    TriggerModes(Vec<crate::mana::CastMode>),
    ApplyCounterDoublersFirst(usize),
    TriggerPermanentTargets(Vec<TargetRef>),
    /// Pay the declaration's locked mana cost; each selected white Phyrexian symbol costs two life.
    PayAttackCost {
        life_symbols: RulesInteger,
        mana: ManaPayment,
    },
    /// Back up the pending declaration so the active player can submit legal attackers.
    RedeclareAttackers,
    /// Reverse the declaration taps while retaining mana abilities and effects.
    RedeclareKeepingMana,
    /// Reverse these specific mana activations, identified by the current declaration.
    RedeclareReversingMana(Vec<crate::game::AttackManaId>),
    DeclareAttackers(Vec<AttackDeclaration>),
    /// CR 508.1e. Members are exact creatures from the accompanying declarations.
    DeclareBandedAttackers {
        attackers: Vec<AttackDeclaration>,
        bands: Vec<Vec<ObjectId>>,
    },
    DeclareBlockers(Vec<BlockDeclaration>),
    AssignCombatDamage(Vec<CombatDamageAssignment>),
    Mulligan(MulliganChoice),
    Bottom(Vec<ObjectId>),
    CopyTargets(CopyTargetChoice),
    DamageOrder(DamageApplication),
    AllocatePrevention(Vec<DamageAllocation>),
    ManaColor(ManaType),
    /// Quantity of the first offered color. The remainder uses the second color.
    SplitMana(RulesInteger),
    Proliferate(Vec<TargetRef>),
    RemoveCounters(BTreeMap<CounterKind, u8>),
    CreatureType(CreatureTypeId),
}
impl From<CopyTargetChoice> for DecisionReply {
    fn from(choice: CopyTargetChoice) -> Self {
        Self::CopyTargets(choice)
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuleCompletion {
    LinkedReturn,
    LandPlay,
    ActivationCost,
    Checks(CheckCompletion),
    Resolution,
    GameEnded,
    ManaAbility,
    TurnWork(WorkCompleted),
}
/// Operation diagnostics and continuation status; state/append belong to the envelope.
// Returned by value from rule progression; boxing Complete would allocate on every step.
#[allow(clippy::large_enum_variant)]
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
    CounterReplacementRequired(CounterReplacementChoice),
    /// Selected object payments cannot legally pay this spell’s cost.
    InvalidCastResource,
    /// The supplied creatures cannot pay a crew or saddle activation cost.
    InvalidPowerContributors,
    /// This permanent already activated a loyalty ability this turn.
    LoyaltyAlreadyActivated,
    /// Not enough loyalty counters to pay the chosen loyalty cost.
    InsufficientLoyalty,
    /// This instance of the source's exhaust ability has been activated.
    AlreadyExhausted,
    /// The printed battlefield or hand restriction does not hold at announcement.
    ActivationConditionNotMet,
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
    TokenDefenderRequired,
    /// Multiple permanents controlled by one player need a simultaneous transform timestamp order.
    TransformTimestampOrderRequired,
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
    /// A spell with split second is on the stack (CR 702.61).
    SplitSecond,
    OpponentActionProhibited,
    SpellCastLimitReached,
    ChosenNameProhibitsCasting,
    CastingFromZoneProhibited,
    ArtifactActivationProhibited,
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
    /// Commander identities were not supplied at the trusted opening boundary.
    MissingCommanderColorIdentity,
    /// The chosen hybrid payment is not one of the ability’s two colors.
    IllegalHybridManaPayment,
    /// The chosen production is not one of the ability’s printed bundles.
    IllegalManaBundle,
    /// A mana ability’s printed activation condition is not satisfied.
    ManaActivationConditionNotMet,
    /// A permanent entering the battlefield needs a color chosen before entry.
    EntryColorChoiceRequired,
    /// The supplied pre-entry color is outside that permanent's domain.
    IllegalEntryColor,
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
/// Permission to play an exact card from a specified zone.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct CardPlayPermission {
    zone: CardPlayZone,
    object: ObjectId,
    player: SeatId,
    duration: CardPlayDuration,
    any_color: bool,
    free: bool,
    cast_only: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum CardPlayDuration {
    ThroughTurn,
    ThroughNextOwnTurn(SeatId),
    ThroughNextOwnTurnSkipCurrent(SeatId),
    WhileInZone,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum CardPlayZone {
    Exile,
    Graveyard,
    Hand,
}

impl CardPlayZone {
    fn allows(self, zone: ObjectZone) -> bool {
        matches!(
            (self, zone),
            (Self::Exile, ObjectZone::Exile)
                | (Self::Graveyard, ObjectZone::Graveyard(_))
                | (Self::Hand, ObjectZone::Hand(_))
        )
    }
}

/// Active duration-scoped game-rule modification installed by `Instruction::CreateTemporaryRule`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ActiveTemporaryRule {
    pub(crate) id: u64,
    pub(crate) controller: SeatId,
    pub(crate) source_object: Option<ObjectId>,
    pub(crate) source_damage: Option<crate::damage::DamageSource>,
    pub(crate) source_entry: Option<StackEntryId>,
    pub(crate) rule: crate::program::TemporaryRuleKind,
    pub(crate) duration: crate::characteristics::ResolvedDuration,
    pub(crate) resolved_players: BTreeSet<SeatId>,
    pub(crate) resolved_objects: BTreeSet<ObjectId>,
    pub(crate) target: Option<TargetRef>,
    pub(crate) second_target: Option<TargetRef>,
    pub(crate) consumed: bool,
}

/// Resolution facts captured when `Instruction::EmitReflexiveTrigger` fires so the resulting
/// reflexive triggered ability can read values from the parent resolution (CR 603.12).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
struct ReflexiveTriggerContext {
    last_sacrificed_creature: Option<crate::characteristics::PowerToughness>,
    resolution_x: Option<RulesInteger>,
    discarded_types: BTreeSet<CardType>,
    exiled_target: Option<DepartedTarget>,
    last_moved_objects: Vec<ObjectId>,
    last_moved_controllers: Vec<SeatId>,
    last_action_count: Option<RulesInteger>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EffectWindow {
    revision: u64,
    ending_turn: bool,
    current_step: Option<crate::turn::Step>,
    current_turn_number: u64,
    counter_order: counter_order::CounterOrderCursor,
    window: StackWindow,
    programs: Arc<BTreeMap<CardId, Program>>,
    creature_vocabulary: Arc<CreatureTypeVocabulary>,
    name_rules: Arc<spellwake_contracts::names::NameRules>,
    payloads: BTreeMap<StackEntryId, StackPayload>,
    triggers: Box<crate::occurrence::TriggerState>,
    pending_work: Option<PendingWork>,
    /// Players who successfully drew at least one card in the current turn.
    cards_drawn_this_turn: BTreeMap<SeatId, RulesInteger>,
    encore_attack_targets: BTreeMap<ObjectId, SeatId>,
    departed_quest_counts: BTreeMap<ObjectId, RulesInteger>,
    spell_ordinals: Option<BTreeMap<SeatId, RulesInteger>>,
    spell_cast_positions: Option<SpellCastPositions>,
    departed_attack_defenders: Option<BTreeMap<ObjectId, (SeatId, Option<ObjectId>)>>,
    track_artifact_tapped_for_mana: bool,
    next_turn_attack_targets: BTreeMap<SeatId, Vec<ObjectId>>,
    attack_directions: Vec<(SeatId, AttackDirection)>,
    required_planeswalker_attacks: Vec<ObjectId>,
    creatures_died_this_turn: RulesInteger,
    token_creators_this_turn: BTreeSet<SeatId>,
    damage: BTreeMap<ObjectId, RulesInteger>,
    /// Exact battlefield incarnations actually damaged by a temporary exile-if-dies effect.
    damage_exile_until_cleanup: BTreeMap<ObjectId, BTreeSet<(StackEntryId, usize)>>,
    regeneration_shields: BTreeMap<ObjectId, BTreeSet<(StackEntryId, usize)>>,
    // Deathtouch is checked once per dealt event even if destruction is prevented.
    deathtouch_damage: BTreeSet<ObjectId>,
    // Only departed incarnations still referenced by an ability need keyword LKI.
    growth_last_known: BTreeMap<ObjectId, crate::occurrence::SupportedAppearance>,
    extra_turns: Vec<SeatId>,
    additional_combats: usize,
    resume_turn_order_at: Option<SeatId>,
    /// Departed seats whose turn slots were passed before the upcoming untap.
    skipped_turns: BTreeSet<SeatId>,
    departed_damage_sources: BTreeMap<ObjectId, crate::damage::DepartedDamageSource>,
    departed_reflexive_sources: BTreeMap<ObjectId, crate::occurrence::SupportedAppearance>,
    /// Last copiable values of departed permanents whose pending abilities create token copies.
    departed_copy_sources: BTreeMap<ObjectId, Program>,
    departed_saga_sources: Option<BTreeMap<ObjectId, occurrence_flow::SagaSource>>,
    aura_graveyard_returns: BTreeMap<ObjectId, ObjectId>,
    mana: BTreeMap<(SeatId, ManaType), RulesInteger>,
    floating_mana: mana_provenance::ManaProvenance,
    retained_mana: Option<std::collections::VecDeque<ManaProductionRecord>>,
    restricted_mana: BTreeMap<(SeatId, ManaType, crate::mana::ManaRestriction), RulesInteger>,
    // Captured stackless mana triggers waiting for independent color choices.
    triggered_mana_queue: Vec<(
        ObjectId,
        crate::mana::ManaAbilityId,
        SeatId,
        SeatId,
        BTreeSet<ManaType>,
    )>,
    life: crate::life::LifeState,
    // The tracked sets exist only if this game has a program reading them.
    spells_cast_by_player: BTreeMap<SeatId, u8>,
    previous_turn_casts: Option<crate::program::PreviousTurnCastCondition>,
    previous_active_spells: Option<u8>,
    transform_orders: BTreeMap<SeatId, Vec<ObjectId>>,
    untap_preface_done: bool,
    untap_keep_tapped: Option<BTreeSet<ObjectId>>,
    cast_history: Option<CastTurnHistory>,
    recent_graveyard_cards: Option<BTreeSet<ObjectId>>,
    activated_this_turn: BTreeMap<
        (
            ObjectId,
            Option<crate::characteristics::CharacteristicTimestamp>,
            ActivatedAbilityId,
            crate::characteristics::ActivationOriginPath,
        ),
        u8,
    >,
    combat_damaged_players_by_source: BTreeMap<ObjectId, BTreeSet<SeatId>>,
    skip_next_controller_untap: BTreeSet<ObjectId>,
    /// Target, source and continuous control epoch for an ongoing untap restriction.
    untap_locks: BTreeSet<(ObjectId, ObjectId, crate::object_identity::ControlEpoch)>,
    /// Exact graveyard incarnations whose granted flashback expires at cleanup.
    granted_flashback: BTreeSet<ObjectId>,
    must_block_this_turn: BTreeMap<ObjectId, usize>,
    must_be_blocked_this_turn: BTreeMap<ObjectId, usize>,
    creatures_required_to_attack: BTreeMap<ObjectId, usize>,
    flash_through_cleanup: BTreeSet<SeatId>,
    ability_resolutions_this_turn:
        BTreeMap<(ObjectId, crate::characteristics::ActivationOriginPath), u16>,
    chosen_modal_modes_turn: BTreeMap<(ObjectId, crate::program::TriggeredAbilityId), BTreeSet<u8>>,
    chosen_modal_modes_ever: BTreeMap<(ObjectId, crate::program::TriggeredAbilityId), BTreeSet<u8>>,
    temporary_rules: Vec<ActiveTemporaryRule>,
    next_temp_rule_id: u64,
    uncounterable_stack_entries: BTreeSet<StackEntryId>,
    uncounterable_by_colors: BTreeMap<StackEntryId, crate::program::ColorMask>,
    pending_reflexive_context: BTreeMap<TriggerInstanceId, ReflexiveTriggerContext>,
    source_cast_uses: BTreeSet<(ObjectId, crate::characteristics::StaticAbilityId)>,
    source_cast_pending:
        BTreeMap<StackEntryId, Vec<(ObjectId, crate::characteristics::StaticAbilityId)>>,
    track_combat_prices: bool,
    track_discard_prices: bool,
    commanders: BTreeMap<CardId, CommanderRecord>,
    commander_color_identities: Option<BTreeMap<SeatId, BTreeSet<crate::characteristics::Color>>>,
    goaded: BTreeMap<ObjectId, BTreeSet<SeatId>>,
    entry_color_choices: BTreeMap<ObjectId, ManaType>,
    entry_creature_type_choices:
        BTreeMap<ObjectId, spellwake_contracts::creature_types::CreatureTypeId>,
    entry_choices: BTreeMap<ObjectId, crate::land::EntryChoice>,
    prospective_entry_controllers: BTreeMap<ObjectId, SeatId>,
    resolution_entry_choices: BTreeMap<ObjectId, crate::land::EntryChoice>,
    prospective_devour_counters: BTreeMap<ObjectId, usize>,
    prospective_entry_copies: BTreeMap<ObjectId, entry_copy::EntryCopyResult>,
    copy_entry_counters: BTreeMap<ObjectId, usize>,
    resolution_aura_choices: BTreeMap<ObjectId, TargetRef>,
    settled_entry_destinations: BTreeMap<ObjectId, crate::program::PublicZoneDestination>,
    pending_entry_reveals: Vec<GameObject>,
    linked_exiles: BTreeMap<ObjectId, Vec<ObjectId>>,
    independent_exile_links:
        BTreeMap<(ObjectId, crate::program::LinkedExileGroup), BTreeSet<ObjectId>>,
    linked_search_results: BTreeMap<ObjectId, Vec<ObjectId>>,
    monarch_linked_exiles: Vec<(ObjectId, SeatId)>,
    linked_return_work: Option<LinkedReturnWork>,
    deferred_selected_activation: Option<(
        selected_activation_cost::SelectedActivationOwner,
        ManaPaymentRecord,
    )>,
    deferred_activation: Option<DeferredActivation>,
    deferred_cast: Option<DeferredCast>,
    cast_cost_replacement: Option<CastCostReplacementWork>,
    settled_cast_cost_madness: BTreeSet<ObjectId>,
    settled_cast_cost_destinations: BTreeMap<ObjectId, crate::program::PublicZoneDestination>,
    settled_cast_cost_order: Vec<ObjectId>,
    deferred_mana_cast: Option<DeferredManaCast>,
    completed_deferred_receipts: Option<(
        ManaPaymentRecord,
        Option<crate::activated::AbilityActivationRecord>,
    )>,
    completed_search_shuffle: Option<(EntropyTape, ShuffleRecord)>,
    completed_land_play: Option<LandPlayRecord>,
    // Every zone entry allocates a new identity. This also covers tokens, copied permanents,
    // and unusual entry effects, without mistaking a control change for an entry.
    turn_start_incarnation: ObjectId,
    damage_to_players_this_turn: Option<BTreeMap<SeatId, RulesInteger>>,
    warped: BTreeMap<ObjectId, u64>,
    plotted: BTreeMap<ObjectId, u64>,
    /// Permission is bound to an exile incarnation and player, and cleared on the next turn.
    free_exiled_cards: BTreeSet<(ObjectId, SeatId)>,
    free_exiled_noncreature_spells: BTreeSet<(ObjectId, SeatId)>,
    typed_exile_cohorts: BTreeMap<StackEntryId, TypedExileCohort>,
    typed_exile_pending: BTreeMap<StackEntryId, (StackEntryId, CardType)>,
    draw_replacement: Option<Box<draw_replacement::DrawReplacement>>,
    pending_life_replacement: Option<Box<event_replacement::PendingLifeReplacement>>,
    active_event_replacements:
        BTreeSet<(Option<ObjectId>, crate::characteristics::StaticAbilityId)>,
    adventured: BTreeSet<(ObjectId, SeatId)>,
    card_play_permissions: BTreeSet<CardPlayPermission>,
    temporary_any_color_exile_cast: BTreeSet<(ObjectId, SeatId)>,
    any_type_exile_cast: BTreeSet<(ObjectId, SeatId)>,
    exile_play_until_next: BTreeMap<ObjectId, (ObjectId, SeatId)>,
    /// Exile incarnations concealed from everyone except their authorized viewer.
    face_down_exile: BTreeMap<ObjectId, SeatId>,
    token_linked_exiles: BTreeMap<ObjectId, Option<ObjectId>>,
    source_created_tokens: BTreeMap<ObjectId, BTreeSet<ObjectId>>,
    poison: BTreeMap<SeatId, RulesInteger>,
    combat_rules: CombatRules,
    combat: Option<CombatState>,
    characteristics: Box<CharacteristicState>,
    damage_state: Box<DamageState>,
    failed_draws: BTreeSet<SeatId>,
    state_based_losses: BTreeSet<SeatId>,
    pub(crate) won_by_effect: Option<(SeatId, ObjectId)>,
}
#[derive(Debug, Clone, PartialEq, Eq, Default)]
struct SpellCastPositions {
    count: u64,
    positions: BTreeMap<StackEntryId, u64>,
}

/// Events in the current scheduled turn used to determine a conditional casting price.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CastTurnHistory {
    /// Incarnations created by a discard, retained for the remainder of this turn.
    pub discarded: BTreeMap<ObjectId, SeatId>,
    /// Creature types of sources that dealt combat damage to a player, captured at the damage event.
    pub combat_types: BTreeMap<SeatId, BTreeSet<CreatureTypeId>>,
    /// Controllers of Assassin creatures or commanders that dealt combat damage to a player.
    pub freerunning: BTreeSet<SeatId>,
    pub spells: RulesInteger,
    pub grave_permanents: RulesInteger,
    pub casters: BTreeSet<SeatId>,
    pub lost_life: BTreeSet<SeatId>,
}
impl Default for CastTurnHistory {
    fn default() -> Self {
        Self {
            discarded: BTreeMap::new(),
            combat_types: BTreeMap::new(),
            freerunning: BTreeSet::new(),
            spells: 0.into(),
            grave_permanents: 0.into(),
            casters: BTreeSet::new(),
            lost_life: BTreeSet::new(),
        }
    }
}
/// The state, payment and current rules append are adopted together.
pub type CastTransition = RulesTransition<EffectWindow, ManaPaymentRecord>;
/// Fixed production transports the same append as its successful state.
pub type ManaActivationTransition = RulesTransition<EffectWindow, ManaProductionRecord>;
/// Immutable executor-issued token for an ability that has already paid its tap.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManaColorChoice {
    revision: u64,
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
/// The legal new defenders for an attacking creature during resolution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttackDefenderFrame {
    revision: u64,
    entry: StackEntryId,
    instruction: usize,
    chooser: SeatId,
    attacker: ObjectId,
    defenders: BTreeMap<TargetRef, SeatId>,
}
impl AttackDefenderFrame {
    /// Controller of the resolving ability.
    pub fn chooser(&self) -> SeatId {
        self.chooser
    }
    /// Exact attacking incarnation whose defender may change.
    pub fn attacker(&self) -> ObjectId {
        self.attacker
    }
    /// Seats and planeswalkers this attacker can be redirected to.
    pub fn defenders(&self) -> impl Iterator<Item = TargetRef> + '_ {
        self.defenders.keys().copied()
    }
}
/// Direction around the table starting at the attacking player's seat.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum AttackDirection {
    Left,
    Right,
}
/// Select one planeswalker subtype from CR 205.3j.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlaneswalkerTypeFrame {
    revision: u64,
    entry: StackEntryId,
    instruction: usize,
    chooser: SeatId,
}
impl PlaneswalkerTypeFrame {
    pub fn chooser(&self) -> SeatId {
        self.chooser
    }
}
/// Choose the direction in which players may declare attacks until the next turn of the ability controller.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttackDirectionFrame {
    revision: u64,
    entry: StackEntryId,
    instruction: usize,
    chooser: SeatId,
}
impl AttackDirectionFrame {
    pub fn chooser(&self) -> SeatId {
        self.chooser
    }
}

/// Public seats eligible for a choice of opponent made as an instruction resolves.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpponentChoiceFrame {
    revision: u64,
    entry: StackEntryId,
    instruction: usize,
    chooser: SeatId,
    opponents: Vec<SeatId>,
}
impl OpponentChoiceFrame {
    /// Controller making the choice.
    pub fn chooser(&self) -> SeatId {
        self.chooser
    }
    /// Eligible opponents in turn order.
    pub fn opponents(&self) -> &[SeatId] {
        &self.opponents
    }
}
/// The resolving ability's non-targeted choice of a damage recipient.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DamageRecipientChoiceFrame {
    revision: u64,
    entry: StackEntryId,
    instruction: usize,
    chooser: SeatId,
    recipients: Vec<TargetRef>,
}
impl DamageRecipientChoiceFrame {
    /// The controller selecting the recipient.
    pub fn chooser(&self) -> SeatId {
        self.chooser
    }
    /// Exact recipients available to the resolving instruction.
    pub fn recipients(&self) -> &[TargetRef] {
        &self.recipients
    }
}

/// Select controlled permanents from which to remove one loyalty counter each.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoyaltyRemovalChoice {
    revision: u64,
    entry: StackEntryId,
    instruction: usize,
    chooser: SeatId,
    objects: Vec<ObjectId>,
}
impl LoyaltyRemovalChoice {
    pub fn chooser(&self) -> SeatId {
        self.chooser
    }
    pub fn objects(&self) -> &[ObjectId] {
        &self.objects
    }
    pub fn revision(&self) -> u64 {
        self.revision
    }
    pub fn entry(&self) -> StackEntryId {
        self.entry
    }
    pub fn instruction(&self) -> usize {
        self.instruction
    }
}
/// Choice of a mana color made while an instruction resolves.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolutionManaColorChoice {
    revision: u64,
    entry: StackEntryId,
    instruction: usize,
    chooser: SeatId,
    colors: BTreeSet<ManaType>,
    split: Option<(ManaType, ManaType, RulesInteger)>,
}
impl ResolutionManaColorChoice {
    pub fn chooser(&self) -> SeatId {
        self.chooser
    }
    pub fn colors(&self) -> &BTreeSet<ManaType> {
        &self.colors
    }
    /// The first color, the remainder color and the total when allocating mana.
    pub fn split(&self) -> Option<(ManaType, ManaType, &RulesInteger)> {
        self.split
            .as_ref()
            .map(|(first, second, total)| (*first, *second, total))
    }
    pub fn revision(&self) -> u64 {
        self.revision
    }
    pub fn entry(&self) -> StackEntryId {
        self.entry
    }
    pub fn instruction(&self) -> usize {
        self.instruction
    }
}
// Short-lived transition return value for mana activation.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ManaActivationResult {
    Complete {
        production: ManaProductionRecord,
    },
    Pending {
        frame: ManaColorChoice,
    },
    /// A cost replacement or related immediate rule requires a decision.
    PendingRule {
        frame: EffectDecision,
    },
}
pub type ManaActivationProgress = RulesTransition<EffectWindow, ManaActivationResult>;
// Short-lived internal dispatch result for mana color resolution.
#[allow(clippy::large_enum_variant)]
pub(crate) enum ManaColorResolution {
    Complete(ManaActivationTransition),
    Pending(ManaActivationProgress),
    Damage {
        production: ManaProductionRecord,
        progress: RuleProgress,
    },
}
#[derive(Default)]
struct ManaActivationChoices<'a> {
    payment: Option<&'a ManaPayment>,
    bundle: Option<&'a BTreeMap<ManaType, RulesInteger>>,
    cost_objects: Option<&'a [ObjectId]>,
    recipient: Option<SeatId>,
    restricted: Option<&'a [crate::mana::RestrictedManaSpend]>,
}
/// Exactly one owned rules process, never parallel mutable continuation fields.
// Sole active work slot on EffectWindow; Damage/Resolution are the hot variants.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone, PartialEq, Eq)]
enum PendingWork {
    ManaSacrifice(Box<ManaSacrificeWork>),
    PermanentEntry(Box<PermanentEntryWork>),
    CastRecovery(Box<CastRecoveryFrame>),
    BattlefieldEntry(Box<BattlefieldEntryWork>),
    SearchEntry(Box<library_search::SearchEntryWork>),
    LandPlay(Box<LandPlayWork>),
    ActivationCost(Box<ActivationCostWork>),
    SelectedActivationCost(Box<selected_activation_cost::SelectedActivationCostWork>),
    GraveyardExile(Box<graveyard_exile::GraveyardExileWork>),
    GraveyardShuffle(Box<graveyard_shuffle::GraveyardShuffleWork>),
    ImpulseReplacement(Box<ImpulseReplacementWork>),
    LibraryTop(Box<LibraryTopWork>),
    LookTop(Box<look_top::LookTopWork>),
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
#[derive(Debug, Clone, PartialEq, Eq)]
struct ManaSacrificeWork {
    source: GameObject,
    sacrificed: GameObject,
    ability: crate::mana::ManaAbilityRef,
    actor: SeatId,
    parent: ManaParent,
    output: ManaOutput,
    source_tap: SacrificedManaSource,
    batch: Option<ZoneReplacementBatch>,
}

/// Last-known type membership after the tap cost and before the source leaves.
#[derive(Debug, Clone, PartialEq, Eq)]
struct SacrificedManaSource {
    tapped: bool,
    was_land: bool,
    was_artifact: bool,
    was_snow: bool,
    was_treasure: bool,
    was_creature: bool,
    names: spellwake_contracts::names::ObjectNames,
}

/// One payload owner for instruction bodies and ordinary original permanent
/// spells. Permanent copies/tokens are not represented by this tag.
#[derive(Debug, Clone, PartialEq, Eq)]
enum StackPayload {
    Instructions(Box<InstructionPayload>),
    Permanent,
}
#[derive(Debug, Clone, PartialEq, Eq)]
struct RevealedCostCard {
    object: ObjectId,
    last_power: RulesInteger,
    mana_value: RulesInteger,
}
#[derive(Debug, Clone, PartialEq, Eq)]
struct InstructionPayload {
    /// Whether this spell satisfied its printed actual-mana-spent threshold. Copies start false.
    met_mana_threshold: bool,
    /// Chosen player for each target of a for-each-player clause.
    target_controllers: BTreeMap<TargetId, SeatId>,
    /// Last known power of a single creature sacrificed as this ability's cost.
    sacrificed_creature_power: Option<RulesInteger>,
    sacrificed_creature_toughness: Option<RulesInteger>,
    exiled_cost_creature: Option<(ObjectId, RulesInteger)>,
    exiled_activation_copy: Option<Box<Program>>,
    granting_damage_source: Option<Box<DamageSource>>,
    sacrificed_creature_subtypes: BTreeSet<CreatureTypeId>,
    paid_creature_taps: usize,
    paid_named_counters: Option<RulesInteger>,
    target_division: BTreeMap<TargetId, u16>,
    activated_ability: Option<crate::characteristics::ActivationOriginPath>,
    myriad: Option<myriad::MyriadChoices>,
    delayed_copy_action: Option<crate::occurrence::DelayedCopyAction>,
    activated_as_loyalty: bool,
    activated_planeswalker_types: BTreeSet<crate::program::PlaneswalkerSubtype>,
    spell_target_mana_value: Option<RulesInteger>,
    copied_mana_value: Option<RulesInteger>,
    departed_source_counters: BTreeMap<CounterKind, RulesInteger>,
    encore_source: Option<Box<crate::occurrence::SupportedAppearance>>,
    source_charge_count: Option<RulesInteger>,
    sacrificed_permanent_mana_value: Option<RulesInteger>,
    revealed_cost_card: Option<RevealedCostCard>,
    splice_target_bodies: BTreeMap<TargetId, ExecutableBody>,
    modes: Vec<modes::SelectedMode>,
    cascade: Option<cascade::CascadeBatch>,
    copy_run: Option<cast_copies::CopyRun>,
    pending_copy_targets: Vec<Option<TargetChoice>>,
    rebound_over_buyback: Option<bool>,
    exile_on_resolve: bool,
    return_to_hand_on_resolve: bool,
    library_on_resolve: Option<crate::program::LibraryPlacement>,
    cannot_be_countered_by_temp: bool,
    reflexive_context: Option<ReflexiveTriggerContext>,
    copiable_traits: Option<CopiableSpellTraits>,
    body: ExecutableBody,
    bindings: ExecutionBindings,
}
/// Instruction spells own their copy values separately from execution bindings.
/// Abilities have no spell traits; Some(empty) remains a nameless ordinary spell.
#[derive(Debug, Clone, PartialEq, Eq)]
struct CopiableSpellTraits {
    cannot_be_countered: bool,
    colors: BTreeSet<crate::characteristics::Color>,
    keywords: BTreeSet<KeywordAbility>,
    card_types: BTreeSet<CardType>,
    creature_subtypes: BTreeSet<CreatureTypeId>,
    spell_subtypes: BTreeSet<crate::program::SpellSubtype>,
    artifact_subtypes: BTreeSet<crate::program::ArtifactSubtype>,
    enchantment_subtypes: BTreeSet<crate::program::EnchantmentSubtype>,
    names: spellwake_contracts::names::ObjectNames,
    supertypes: BTreeSet<crate::program::Supertype>,
    triggered: Vec<crate::program::TriggeredAbility>,
}
/// Borrowed projection of the existing original-or-owned trait source.
struct CopiableSpellTraitsRef<'a> {
    cannot_be_countered: bool,
    keywords: &'a BTreeSet<KeywordAbility>,
    names: &'a spellwake_contracts::names::ObjectNames,
    supertypes: &'a BTreeSet<crate::program::Supertype>,
    card_types: &'a BTreeSet<CardType>,
    creature_subtypes: &'a BTreeSet<CreatureTypeId>,
    spell_subtypes: &'a BTreeSet<crate::program::SpellSubtype>,
    artifact_subtypes: &'a BTreeSet<crate::program::ArtifactSubtype>,
    enchantment_subtypes: &'a BTreeSet<crate::program::EnchantmentSubtype>,
    triggered: &'a [crate::program::TriggeredAbility],
}
impl CopiableSpellTraits {
    fn from_program(program: &Program) -> Self {
        Self {
            cannot_be_countered: program.cannot_be_countered,
            colors: if program.printed_keywords.contains(&KeywordAbility::Devoid) {
                BTreeSet::new()
            } else {
                program.defined_colors()
            },
            keywords: program.printed_keywords.clone(),
            card_types: program.card_types.clone(),
            creature_subtypes: program.creature_subtypes.clone(),
            spell_subtypes: program.spell_subtypes.clone(),
            artifact_subtypes: program.artifact_subtypes.clone(),
            enchantment_subtypes: program.enchantment_subtypes.clone(),
            names: program.names.clone(),
            supertypes: program.supertypes.clone(),
            triggered: program.triggered.clone(),
        }
    }
}
impl StackPayload {
    fn from_body(body: ExecutableBody, copiable_traits: Option<CopiableSpellTraits>) -> Self {
        Self::Instructions(Box::new(InstructionPayload {
            met_mana_threshold: false,
            target_controllers: BTreeMap::new(),
            revealed_cost_card: None,
            sacrificed_permanent_mana_value: None,
            activated_ability: None,
            myriad: None,
            delayed_copy_action: None,
            activated_as_loyalty: false,
            activated_planeswalker_types: BTreeSet::new(),
            spell_target_mana_value: None,
            copied_mana_value: None,
            departed_source_counters: BTreeMap::new(),
            encore_source: None,
            source_charge_count: None,
            sacrificed_creature_power: None,
            sacrificed_creature_toughness: None,
            exiled_cost_creature: None,
            exiled_activation_copy: None,
            granting_damage_source: None,
            sacrificed_creature_subtypes: BTreeSet::new(),
            paid_creature_taps: 0,
            paid_named_counters: None,
            target_division: BTreeMap::new(),
            splice_target_bodies: BTreeMap::new(),
            modes: Vec::new(),
            rebound_over_buyback: None,
            exile_on_resolve: false,
            return_to_hand_on_resolve: false,
            library_on_resolve: None,
            cannot_be_countered_by_temp: false,
            reflexive_context: None,
            cascade: None,
            copy_run: None,
            pending_copy_targets: Vec::new(),
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
    /// Printed keywords owned by a copied spell.
    pub copied_keywords: Option<&'a BTreeSet<KeywordAbility>>,
    /// Last-known power of a single creature sacrificed to activate this entry.
    pub sacrificed_creature_power: Option<&'a RulesInteger>,
    pub paid_creature_taps: usize,
    /// Actual retained capture bindings, not reconstructed from current objects.
    pub bindings: Option<&'a ExecutionBindings>,
}
/// CR 608.2h last-known information for a targeted object during resolution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct TargetLki {
    pub(super) owner: SeatId,
    pub(super) controller: Option<SeatId>,
    pub(super) mana_value: RulesInteger,
    pub(super) power: Option<RulesInteger>,
    pub(super) toughness: Option<RulesInteger>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ClashState {
    chooser: SeatId,
    opponent: SeatId,
    chooser_card: Option<ObjectId>,
    opponent_card: Option<ObjectId>,
    stage: u8,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct DiscoverState {
    candidate: Option<ObjectId>,
    exiled: Vec<ObjectId>,
    awaiting_random_bottom: bool,
    bottom_order: Option<Vec<ObjectId>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ResolutionResults {
    target_lki: BTreeMap<ObjectId, TargetLki>,
    sampled_graveyard_cards: Option<BTreeSet<ObjectId>>,
    revealed_top_types: BTreeSet<CardType>,
    revealed_top_card: Option<ObjectId>,
    last_amassed_army: Option<ObjectId>,
    countered_spell_controller: Option<(StackEntryId, SeatId)>,
    counter_target_types: BTreeSet<CardType>,
    shuffled_source_owner: Option<SeatId>,
    destroyed_target_controller: Option<SeatId>,
    destroyed_cards_to_graveyard: Vec<ObjectId>,
    destruction_target_toughness: Option<RulesInteger>,
    destruction_target_controller: Option<SeatId>,
    power_damage_cohort: Option<(ObjectId, Vec<ObjectId>)>,
    destruction_target_subtypes: BTreeSet<CreatureTypeId>,
    last_sacrifice: bool,
    last_action_succeeded: bool,
    last_action_count: Option<RulesInteger>,
    discarded_types: BTreeSet<CardType>,
    moved_library_types: BTreeSet<CardType>,
    milled_mana_values: Vec<RulesInteger>,
    selected_graveyard_exile_creatures: usize,
    last_sacrificed_creature: Option<crate::characteristics::PowerToughness>,
    returned_target_permanent: Option<ObjectId>,
    last_die_roll: Option<RulesInteger>,
    last_die_rolls: Vec<u16>,
    die_roll_ready: bool,
    last_coin_flips_won: Option<RulesInteger>,
    last_coin_flip_won: Option<bool>,
    coin_flip_ready: bool,
    chosen_card_type: Option<CardType>,
    chosen_basic_land_type: Option<crate::land::LandType>,
    chosen_card_name: Option<String>,
    last_moved_controllers: Vec<SeatId>,
    last_moved_objects: Vec<ObjectId>,
    chosen_number: Option<RulesInteger>,
    chosen_option_index: Option<u8>,
    vote_counts: Vec<u16>,
    winning_vote_option: Option<u8>,
    pending_voters: std::collections::VecDeque<SeatId>,
    voters_initialized: bool,
    pending_option_players: std::collections::VecDeque<SeatId>,
    option_players_initialized: bool,
    last_nonland_discarded: u16,
    clash_state: Option<ClashState>,
    discover_state: Option<DiscoverState>,
    manifest_dread_candidates: Vec<ObjectId>,
    /// Current loop participant bound by `Instruction::ForEachPlayer` / `OfferAnyPlayer`.
    loop_player: Option<SeatId>,
    /// Total damage prevented during this resolution (CR 615.6, e.g. "for each 1 damage prevented this way").
    damage_prevented_this_resolution: RulesInteger,
    /// Whether any damage was dealt during this resolution (e.g. "if damage is dealt this way").
    dealt_damage_this_resolution: bool,
}
impl Default for ResolutionResults {
    fn default() -> Self {
        Self {
            target_lki: BTreeMap::new(),
            sampled_graveyard_cards: None,
            revealed_top_types: BTreeSet::new(),
            revealed_top_card: None,
            last_amassed_army: None,
            countered_spell_controller: None,
            counter_target_types: BTreeSet::new(),
            shuffled_source_owner: None,
            destroyed_target_controller: None,
            destroyed_cards_to_graveyard: Vec::new(),
            destruction_target_toughness: None,
            destruction_target_controller: None,
            power_damage_cohort: None,
            destruction_target_subtypes: BTreeSet::new(),
            last_sacrifice: false,
            last_action_succeeded: false,
            last_action_count: None,
            discarded_types: BTreeSet::new(),
            moved_library_types: BTreeSet::new(),
            milled_mana_values: Vec::new(),
            selected_graveyard_exile_creatures: 0,
            last_sacrificed_creature: None,
            returned_target_permanent: None,
            last_die_roll: None,
            last_die_rolls: Vec::new(),
            die_roll_ready: false,
            last_coin_flips_won: None,
            last_coin_flip_won: None,
            coin_flip_ready: false,
            chosen_card_type: None,
            chosen_basic_land_type: None,
            chosen_card_name: None,
            last_moved_controllers: Vec::new(),
            last_moved_objects: Vec::new(),
            chosen_number: None,
            chosen_option_index: None,
            vote_counts: Vec::new(),
            winning_vote_option: None,
            pending_voters: std::collections::VecDeque::new(),
            voters_initialized: false,
            pending_option_players: std::collections::VecDeque::new(),
            option_players_initialized: false,
            last_nonland_discarded: 0,
            clash_state: None,
            discover_state: None,
            manifest_dread_candidates: Vec::new(),
            loop_player: None,
            damage_prevented_this_resolution: 0.into(),
            dealt_damage_this_resolution: false,
        }
    }
}
/// Points into the owned payload. Tokens never own another mutable program counter.
#[derive(Debug, Clone, PartialEq, Eq)]
struct ResolutionCursor {
    chosen_opponent: Option<SeatId>,
    chosen_damage_recipient: Option<TargetRef>,
    damage_offer_opponents: Option<std::collections::VecDeque<SeatId>>,
    vowed_creatures: BTreeSet<ObjectId>,
    meld_pair: Option<[(ObjectId, Option<CardId>); 2]>,
    manifest_remaining: Option<RulesInteger>,
    random_selection: Option<ObjectId>,
    results: ResolutionResults,
    revealed_piles: Option<Box<revealed_piles::RevealedPiles>>,
    chosen_effect_color: Option<ManaType>,
    chosen_option_index: Option<u8>,
    chosen_distinct_colors: Vec<ManaType>,
    chosen_mana_split: Option<RulesInteger>,
    tempting_token_offer: Option<token_offers::TemptingTokenOffer>,
    destroyed_permanents: Option<usize>,
    chosen_loyalty_removals: Option<Vec<ObjectId>>,
    discarded_nonland_mana_value: Option<RulesInteger>,
    last_created_tokens: Vec<ObjectId>,
    chosen_token_equipment: Option<ObjectId>,
    found_spells: Vec<ObjectId>,
    largest_discard: Option<usize>,
    impulse: ImpulseState,
    exiled_target: Option<DepartedTarget>,
    remembered_permanent: Option<(SeatId, RulesInteger)>,
    entry: StackEntryId,
    instruction: usize,
    values: ResolutionValues,
}
#[derive(Debug, Clone, PartialEq, Eq)]
struct ActivationCostWork {
    batch: ZoneReplacementBatch,
    activation: AbilityActivationRecord,
    movement_before: GameObject,
    receipt: ManaPaymentRecord,
}
#[derive(Debug, Clone, PartialEq, Eq)]
struct BattlefieldEntryWork {
    cursor: ResolutionCursor,
    object: ObjectId,
    controller: SeatId,
    reason: crate::occurrence::ZoneReason,
    action: crate::program::ZoneActionKind,
    batch: ZoneReplacementBatch,
}
#[derive(Debug, Clone, PartialEq, Eq)]
struct LandPlayWork {
    actor: SeatId,
    land: ObjectId,
    turn: TurnScheduler,
    choice: Option<crate::land::EntryChoice>,
    batch: ZoneReplacementBatch,
}
#[derive(Debug, Clone, PartialEq, Eq)]
struct PermanentEntryWork {
    entry: crate::stack_window::StackEntry,
    object: ObjectId,
    batch: ZoneReplacementBatch,
}
#[derive(Debug, Clone, PartialEq, Eq)]
struct ImpulseReplacementWork {
    cursor: ResolutionCursor,
    objects: Vec<ObjectId>,
    play_permission: Option<SeatId>,
    face_down_viewer: Option<SeatId>,
    return_to_hand: bool,
    bottom_from_exile: bool,
    graveyard_orders: BTreeMap<SeatId, Vec<ObjectId>>,
    batch: ZoneReplacementBatch,
}
#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct ImpulseState {
    revealed_library_card: Option<ObjectId>,
    repeated_permanent: Option<CardId>,
    nonland_search: Option<Box<NonlandSearch>>,
    rewrite: Option<Box<library_rewrite::LibraryRewrite>>,
    contribution_seats: Option<std::collections::VecDeque<SeatId>>,
    contributions: Option<RulesInteger>,
    votes: Vec<(SeatId, ObjectId)>,
    sacrificed_types: Option<BTreeSet<CreatureTypeId>>,
    kin_reveal: Option<Box<library_rewrite::KinReveal>>,
    flickered: Vec<ObjectId>,
    milled: Vec<ObjectId>,
    exiled_milled_power: Option<RulesInteger>,
    transformed_controllers: std::collections::VecDeque<SeatId>,
    shuffle_after_reveal: bool,

    exiled: Option<ObjectId>,
    started: bool,
    decided: bool,
    cast: bool,
}
#[derive(Debug, Clone, PartialEq, Eq)]
struct TypedExileCohort {
    player: SeatId,
    cards: BTreeSet<ObjectId>,
    used: BTreeSet<CardType>,
}
#[derive(Debug, Clone, PartialEq, Eq)]
struct NonlandSearch {
    phase: NonlandSearchPhase,
    entered_hand: usize,
}
#[derive(Debug, Clone, PartialEq, Eq)]
enum NonlandSearchPhase {
    Exile,
    ToHand { object: ObjectId, repeat: bool },
    Done,
}
/// Last known battlefield values captured before this resolution exiled its target.
#[derive(Debug, Clone, PartialEq, Eq)]
struct DepartedTarget {
    controller: SeatId,
    power: Option<RulesInteger>,
}
type ResolutionValues = BTreeMap<CreatureTypeSlot, CreatureTypeId>;

/// Damage temporarily owns the complete paused process, including its local values.
#[derive(Debug, Clone, PartialEq, Eq)]
struct DamageWork {
    batch: ActiveDamageBatch,
    parent: DamageParent,
    original_origin: DamageOrigin,
    reflection: Option<Box<crate::damage::DamageReflection>>,
    followup_tails: Vec<VecDeque<damage_flow::DamageFollowup>>,
    followups: VecDeque<damage_flow::DamageFollowup>,
    results_committed: bool,
    followup_zone: Option<Box<damage_flow::DamageZoneFollowup>>,
}
#[derive(Debug, Clone, PartialEq, Eq)]
enum DamageParent {
    Resolution(ResolutionCursor),
    Combat,
    Mana(ManaParent),
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
    ManaAbility,
}

/// Executor-issued choice domain; resolution values remain owned by the cursor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreatureTypeChoice {
    revision: u64,
    entry: StackEntryId,
    instruction: usize,
    slot: CreatureTypeSlot,
    chooser: SeatId,
    vocabulary: CreatureVocabularyId,
    excluding: BTreeSet<CreatureTypeId>,
    previously_named: Vec<CreatureTypeId>,
}
impl CreatureTypeChoice {
    /// Previously announced choices in the current multiplayer procedure.
    pub fn previously_named(&self) -> &[CreatureTypeId] {
        &self.previously_named
    }
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
    pub(crate) fn has_pending_cost_or_linked(&self) -> bool {
        self.linked_return_work.is_some() || self.cast_cost_replacement.is_some()
    }
    pub(crate) fn has_deferred_payment(&self) -> bool {
        self.deferred_selected_activation.is_some()
            || self.deferred_activation.is_some()
            || self.deferred_cast.is_some()
            || self.deferred_mana_cast.is_some()
            || matches!(
                self.pending_work,
                Some(PendingWork::ActivationCost(_) | PendingWork::SelectedActivationCost(_))
            )
    }
    /// Cards selected by the searching player whose battlefield entries are
    /// waiting for replacement decisions. This privileged view is private to the
    /// returned chooser and does not authorize disclosure of other library cards.
    pub fn pending_search_entry_subjects(&self) -> Option<(SeatId, Vec<ObjectId>)> {
        if let Some(work) = &self.linked_return_work
            && let LinkedReturnOrigin::LibraryTop { player, .. }
            | LinkedReturnOrigin::Hand { player, .. } = work.origin
        {
            return Some((player, work.objects.clone()));
        }
        if let Some(work) = self
            .resolution_cursor()
            .and_then(|cursor| cursor.revealed_piles.as_ref())
        {
            return Some((self.window.resolving()?.controller(), work.cards.clone()));
        }
        let Some(PendingWork::SearchEntry(work)) = &self.pending_work else {
            return None;
        };
        Some((
            work.actor,
            work.moves
                .iter()
                .filter_map(|(id, destination)| {
                    (matches!(destination, ObjectDestination::Battlefield { .. })
                        || work.action == crate::program::ZoneActionKind::EffectSearchToGraveyard
                            && *destination == ObjectDestination::Graveyard)
                        .then_some(*id)
                })
                .collect(),
        ))
    }
    pub(crate) fn take_completed_land_play(&mut self) -> Option<LandPlayRecord> {
        self.completed_land_play.take()
    }
    pub(crate) fn take_completed_search_shuffle(&mut self) -> Option<(EntropyTape, ShuffleRecord)> {
        self.completed_search_shuffle.take()
    }
    pub(crate) fn take_completed_deferred_receipts(
        &mut self,
    ) -> Option<(
        ManaPaymentRecord,
        Option<crate::activated::AbilityActivationRecord>,
    )> {
        self.completed_deferred_receipts.take()
    }
    /// The next mandatory choice for a linked return, before any further rules work.
    ///
    /// # Errors
    /// Returns an error if the retained batch can no longer be evaluated.
    pub fn pending_linked_return_decision(&self) -> Result<Option<EffectDecision>, EffectError> {
        self.linked_return_decision()
    }
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
        let mut commander_chosen_colors = BTreeMap::new();
        for mut seed in cards {
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
            if let Some(chosen) = seed
                .program
                .commander_color_choice_mut()
                .and_then(Option::take)
            {
                if seed.destination != ObjectDestination::Command {
                    return Err(EffectError::InvalidSetup);
                }
                commander_chosen_colors.insert(seed.card, chosen);
            }
            if seed.program.nightbound_back.is_some() {
                objects.daybound_cards.insert(seed.card);
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
        if programs
            .values()
            .any(entry_counters::needs_creature_damage_pairs)
        {
            characteristics.creature_damage_pairs = Some(BTreeSet::new());
        }
        if programs.values().any(entry_counters::needs_damage_dealt_by) {
            characteristics.track_damage_dealt_by = true;
        }
        characteristics.commander_chosen_colors = commander_chosen_colors;
        if programs.values().any(upkeep::needs_echo_history) {
            characteristics.upkeep_cuts = Some(BTreeMap::new());
        }
        characteristics.register_initial_battlefield(&objects, &initial_battlefield_order)?;
        let life = crate::life::LifeState::new(&order, life).ok_or(EffectError::InvalidSetup)?;
        let evaluated = evaluate(CharacteristicWorld {
            active_player: active,
            step: None,
            participants: &order,
            life: Some(&life),
            entry_creature_types: &BTreeMap::new(),
            mana: &BTreeMap::new(),
            combat: None,
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
        let mut result = Self {
            turn_start_incarnation: window.cards().next_incarnation(),
            revision: 0,
            ending_turn: false,
            current_step: None,
            current_turn_number:1,
            counter_order: counter_order::CounterOrderCursor::default(),
            window,
            commanders: BTreeMap::new(),
            commander_color_identities: None,
            goaded: BTreeMap::new(),
            entry_color_choices: BTreeMap::new(),
            entry_creature_type_choices: BTreeMap::new(),
            entry_choices: BTreeMap::new(),
            prospective_entry_controllers: BTreeMap::new(),
            resolution_entry_choices: BTreeMap::new(),
            prospective_devour_counters: BTreeMap::new(),
            prospective_entry_copies: BTreeMap::new(),
            copy_entry_counters: BTreeMap::new(),
            resolution_aura_choices: BTreeMap::new(),
            settled_entry_destinations: BTreeMap::new(),
            pending_entry_reveals: Vec::new(),
            linked_exiles: BTreeMap::new(),
            independent_exile_links: BTreeMap::new(),
            linked_search_results:BTreeMap::new(),
            monarch_linked_exiles: Vec::new(),
            linked_return_work: None,
            deferred_selected_activation: None,
            deferred_activation: None,
            deferred_cast: None,
            cast_cost_replacement: None,
            settled_cast_cost_madness:BTreeSet::new(),
            settled_cast_cost_destinations: BTreeMap::new(),
            settled_cast_cost_order: Vec::new(),
            deferred_mana_cast: None,
            completed_deferred_receipts: None,
            completed_search_shuffle: None,
            completed_land_play: None,
            damage_to_players_this_turn: programs
                .values()
                .any(entry_counters::needs_damage_history)
                .then(BTreeMap::new),
            track_combat_prices: programs.values().any(|program| matches!(program.conditional_cost, Some(crate::program::ConditionalCastCost::Freerunning(_) | crate::program::ConditionalCastCost::Prowl(_)))),
            recent_graveyard_cards:programs.values().any(|p|entry_counters::definition_or_token_matches(p,|definition|definition.triggered.iter().any(|trigger|trigger.body.target==BodyTarget::RecentlyDiscardedOrFromLibrary))).then(BTreeSet::new),
            track_discard_prices: programs.values().any(|program| matches!(program.conditional_cost, Some(crate::program::ConditionalCastCost::Mayhem(_)))),
            spells_cast_by_player: BTreeMap::new(),
            previous_turn_casts: None,
            previous_active_spells: None,
            transform_orders:BTreeMap::new(),
            untap_preface_done: false,
            untap_keep_tapped: None,
            activated_this_turn: BTreeMap::new(),
            combat_damaged_players_by_source:BTreeMap::new(),
            skip_next_controller_untap: BTreeSet::new(),
            untap_locks: BTreeSet::new(),
            granted_flashback: BTreeSet::new(),
            must_block_this_turn: BTreeMap::new(),
            must_be_blocked_this_turn: BTreeMap::new(),
            creatures_required_to_attack: BTreeMap::new(),
            flash_through_cleanup: BTreeSet::new(),
            ability_resolutions_this_turn: BTreeMap::new(),
            chosen_modal_modes_turn: BTreeMap::new(),
            chosen_modal_modes_ever: BTreeMap::new(),
            temporary_rules: Vec::new(),
            next_temp_rule_id: 1,
            uncounterable_stack_entries: BTreeSet::new(),
            uncounterable_by_colors: BTreeMap::new(),
            pending_reflexive_context: BTreeMap::new(),
            source_cast_uses: BTreeSet::new(),
            source_cast_pending: BTreeMap::new(),
            cast_history: programs
                .values()
                .any(|p| {
                    p.triggered.iter().any(|t| t.cast_copy_keyword().is_some())
                        || matches!(
                            p.conditional_cost,
                            Some(
                                crate::program::ConditionalCastCost::Prowl(_) | crate::program::ConditionalCastCost::Freerunning(_) | crate::program::ConditionalCastCost::Mayhem(_) | crate::program::ConditionalCastCost::Surge(_)
                                    | crate::program::ConditionalCastCost::Spectacle(_)
                            )
                        )
                })
                .then(CastTurnHistory::default),
            spell_cast_positions: programs.values().any(|program| entry_counters::definition_or_token_matches(program, |p| {
                matches!(&p.kind, ProgramKind::Spell(spell) if matches!(spell.body.target,
                    BodyTarget::SingleSpell(filter) if filter.ordinal_this_turn.is_some()))
            })).then(SpellCastPositions::default),
            spell_ordinals:programs.values().any(|p|entry_counters::definition_or_token_matches(p,|p|
                p.triggered.iter().any(|trigger|matches!(trigger.condition.event,crate::program::TriggerEvent::SpellCast {filter,..} if filter.ordinal_this_turn.is_some()))
            )).then(BTreeMap::new),
            departed_attack_defenders:programs.values().any(|program| entry_counters::definition_or_token_matches(program,|definition|
                definition.triggered.iter().any(|trigger|trigger.body.instructions.contains(&Instruction::Myriad) || matches!(trigger.condition.event,crate::program::TriggerEvent::Attacks { mode: crate::program::AttackTriggerMode::OpponentsAtController { minimum: 2 } | crate::program::AttackTriggerMode::FirstThisTurn { highest_life: true }, .. }))
            )).then(BTreeMap::new),
            track_artifact_tapped_for_mana:programs.values().any(|program|entry_counters::definition_or_token_matches(program,|definition|
                definition.triggered.iter().any(|trigger|matches!(trigger.condition.event,crate::program::TriggerEvent::PlayerAction { action: crate::program::PlayerTriggerAction::ArtifactTappedForMana | crate::program::PlayerTriggerAction::PermanentTappedForMana(_), .. })))),
            departed_saga_sources: programs.values().any(|program| entry_counters::definition_or_token_matches(program, |definition| {
                definition.triggered.iter().any(|trigger| matches!(trigger.condition.event, crate::program::TriggerEvent::PlayerAction { action: crate::program::PlayerTriggerAction::SagaFinalChapterResolved, .. }))
            })).then(BTreeMap::new),
            programs: Arc::new(programs),
            creature_vocabulary,
            name_rules,
            payloads: BTreeMap::new(),
            triggers: Box::new(crate::occurrence::TriggerState::default()),
            pending_work: None,
            cards_drawn_this_turn: BTreeMap::new(),
            encore_attack_targets:BTreeMap::new(),
            departed_quest_counts:BTreeMap::new(),
            next_turn_attack_targets:BTreeMap::new(),
            attack_directions:vec![],
            required_planeswalker_attacks:vec![],
            creatures_died_this_turn:0.into(),
            token_creators_this_turn:BTreeSet::new(),
            damage: BTreeMap::new(),
            damage_exile_until_cleanup: BTreeMap::new(),
            regeneration_shields: BTreeMap::new(),
            deathtouch_damage: BTreeSet::new(),
            growth_last_known: BTreeMap::new(),
            extra_turns: Vec::new(),
            additional_combats:0,
            resume_turn_order_at: None,
            skipped_turns: BTreeSet::new(),
            departed_damage_sources: BTreeMap::new(),
            departed_reflexive_sources: BTreeMap::new(),
            departed_copy_sources: BTreeMap::new(),
            mana: pools,
            floating_mana: mana_provenance::ManaProvenance::default(),
            retained_mana: None,
            restricted_mana: BTreeMap::new(),
            triggered_mana_queue: Vec::new(),
            life,
            combat_rules,
            combat: None,
            characteristics: Box::new(characteristics),
            damage_state: Box::new(DamageState::default()),
            warped:BTreeMap::new(),
            plotted: BTreeMap::new(),
            free_exiled_cards:BTreeSet::new(),
            free_exiled_noncreature_spells:BTreeSet::new(),
            typed_exile_cohorts:BTreeMap::new(),typed_exile_pending:BTreeMap::new(),
            draw_replacement: None,
            pending_life_replacement: None,
            active_event_replacements: BTreeSet::new(),
            adventured:BTreeSet::new(),
            card_play_permissions: BTreeSet::new(),
            temporary_any_color_exile_cast:BTreeSet::new(),
            any_type_exile_cast:BTreeSet::new(),
            exile_play_until_next:BTreeMap::new(),
            face_down_exile: BTreeMap::new(),
            token_linked_exiles: BTreeMap::new(),
            source_created_tokens: BTreeMap::new(),
            aura_graveyard_returns: BTreeMap::new(),
            poison: BTreeMap::new(),
            failed_draws: BTreeSet::new(),
            state_based_losses: BTreeSet::new(),
            won_by_effect: None,
        };
        let evaluated = result.evaluated_characteristics()?;
        result.reconcile_day_night(None, &evaluated)?;
        let evaluated = result.evaluated_characteristics()?;
        result.update_city_blessing(&evaluated);
        Ok(result)
    }
    /// Legal opponents of a participant under the current multiplayer rules.
    ///
    /// # Errors
    /// Returns an error if the seat is outside this game.
    pub fn opponents_of(&self, seat: SeatId) -> Result<Vec<SeatId>, CombatError> {
        self.combat_rules
            .opponents(self.window.participants(), seat)
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
    /// Creature type chosen as this exact permanent entered, when applicable.
    pub fn entry_creature_type(
        &self,
        object: ObjectId,
    ) -> Option<spellwake_contracts::creature_types::CreatureTypeId> {
        self.entry_creature_type_choices.get(&object).copied()
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
                ManaParent::Priority
                | ManaParent::CombatPayment(_)
                | ManaParent::Casting(_)
                | ManaParent::Recovery(_, _) => None,
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
                DamageParent::Mana(_) => DamageParentView::ManaAbility,
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
                sacrificed_creature_power: payload.sacrificed_creature_power.as_ref(),
                paid_creature_taps: payload.paid_creature_taps,
                copied_keywords: payload
                    .copiable_traits
                    .as_ref()
                    .map(|traits| &traits.keywords),
                copied_triggered: payload
                    .copiable_traits
                    .as_ref()
                    .map(|traits| traits.triggered.as_slice()),
                bindings: Some(&payload.bindings),
            },
            StackPayload::Permanent => RetainedStackPayload {
                entry,
                body: None,
                sacrificed_creature_power: None,
                paid_creature_taps: 0,
                copied_keywords: None,
                copied_triggered: None,
                bindings: None,
            },
        })
    }
    pub(crate) fn miracle_reveals(&self) -> impl Iterator<Item = &GameObject> {
        let reveal = |body: &ExecutableBody| match body.instructions.as_slice() {
            [Instruction::MiracleCast(object)] => Some(*object),
            _ => None,
        };
        self.waiting_triggers()
            .filter_map(move |trigger| reveal(&trigger.body))
            .chain(
                self.window
                    .entries()
                    .iter()
                    .chain(self.window.resolving())
                    .filter_map(move |entry| self.stack_body(entry.id()).and_then(reveal)),
            )
            .filter_map(move |id| self.window.cards().get(id))
            .filter(|object| matches!(object.zone(), ObjectZone::Hand(_)))
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
    /// Captured instant/sorcery subtypes of an original spell or owned copy.
    pub fn stack_copiable_spell_subtypes(
        &self,
        entry: StackEntryId,
    ) -> Option<&BTreeSet<crate::program::SpellSubtype>> {
        self.stack_copiable_traits(entry)
            .map(|traits| traits.spell_subtypes)
    }
    /// Copiable enchantment subtypes of the current stack spell.
    pub fn stack_copiable_artifact_subtypes(
        &self,
        entry: StackEntryId,
    ) -> Option<&BTreeSet<crate::program::ArtifactSubtype>> {
        self.stack_copiable_traits(entry)
            .map(|traits| traits.artifact_subtypes)
    }
    pub fn stack_copiable_enchantment_subtypes(
        &self,
        entry: StackEntryId,
    ) -> Option<&BTreeSet<crate::program::EnchantmentSubtype>> {
        self.stack_copiable_traits(entry)
            .map(|traits| traits.enchantment_subtypes)
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
    /// Printed keywords of a spell on the stack, including a copy's independent values.
    pub fn stack_copiable_keywords(
        &self,
        entry: StackEntryId,
    ) -> Option<&BTreeSet<KeywordAbility>> {
        self.stack_copiable_traits(entry)
            .map(|traits| traits.keywords)
    }

    fn check_opponents_turn_restriction(
        &self,
        actor: SeatId,
        current: &EvaluatedCharacteristics,
        ability_source: Option<&ObjectCharacteristics>,
    ) -> Result<(), EffectError> {
        let active = self.window.active_player();
        if active == actor
            || ability_source.is_some_and(|source| {
                source.zone != ObjectZone::Battlefield
                    || ![
                        CardType::Artifact,
                        CardType::Creature,
                        CardType::Enchantment,
                    ]
                    .into_iter()
                    .any(|t| source.has_type(t))
            })
        {
            return Ok(());
        }
        if !self
            .combat_rules
            .opponents(self.window.participants(), active)?
            .contains(&actor)
        {
            return Ok(());
        }
        let prohibited = current.objects.values().any(|permanent| {
            permanent.zone == ObjectZone::Battlefield
                && permanent.controller == Some(active)
                && Self::permanent_has_restriction(permanent, |rule| {
                    matches!(
                        rule,
                        crate::characteristics::StaticRestriction::RestrictOpponentsDuringOwnTurn
                    )
                })
        }) || self.temporary_rules.iter().any(|rule| {
            rule.controller == active
                && self.active_temporary_static(rule, current).is_some_and(|ab| {
                    matches!(
                        ab,
                        crate::characteristics::StaticContinuousAbility::Restriction {
                            rule: crate::characteristics::StaticRestriction::RestrictOpponentsDuringOwnTurn,
                            ..
                        }
                    )
                })
        });
        if prohibited {
            Err(EffectError::OpponentActionProhibited)
        } else {
            Ok(())
        }
    }
    fn check_artifact_activation_restriction(
        evaluated: &EvaluatedCharacteristics,
        source: &ObjectCharacteristics,
    ) -> Result<(), EffectError> {
        if source.zone == ObjectZone::Battlefield
            && source.has_type(CardType::Artifact)
            && evaluated.objects.values().any(|permanent| {
                permanent.zone == ObjectZone::Battlefield
                    && Self::permanent_has_restriction(permanent, |rule| {
                        matches!(
                            rule,
                            crate::characteristics::StaticRestriction::PreventArtifactActivations
                        )
                    })
            })
        {
            return Err(EffectError::ArtifactActivationProhibited);
        }
        Ok(())
    }

    /// Return the active static ability for a temporary rule if it is an unconditional
    /// `TemporaryStatic` or a `ConditionalTemporaryStatic` whose condition currently holds.
    pub(super) fn active_temporary_static<'a>(
        &self,
        active: &'a ActiveTemporaryRule,
        evaluated: &EvaluatedCharacteristics,
    ) -> Option<&'a crate::characteristics::StaticContinuousAbility> {
        if active.consumed {
            return None;
        }
        match &active.rule {
            crate::program::TemporaryRuleKind::TemporaryStatic(ab) => Some(ab.as_ref()),
            crate::program::TemporaryRuleKind::ConditionalTemporaryStatic {
                condition,
                ability,
            } => self
                .eval_static_source_condition_at(
                    evaluated,
                    condition,
                    active.source_object,
                    active.controller,
                    None,
                )
                .then_some(ability.as_ref()),
            _ => None,
        }
    }

    /// Check whether a battlefield permanent has a matching restriction either from its active
    /// printed static abilities or from `granted_restrictions` applied in layer 6.
    pub(super) fn permanent_has_restriction(
        permanent: &ObjectCharacteristics,
        mut predicate: impl FnMut(&crate::characteristics::StaticRestriction) -> bool,
    ) -> bool {
        permanent.active_battlefield_rules().any(|ab| {
            matches!(
                ab,
                crate::characteristics::StaticContinuousAbility::Restriction { rule, .. }
                    if predicate(rule)
            )
        }) || permanent.granted_restrictions.iter().any(predicate)
    }

    /// Resolve a card type chosen as `source` entered the battlefield via `ChooseCardType`.
    pub(super) fn entry_chosen_card_type(&self, source: ObjectId) -> Option<CardType> {
        if let Some(entry_tap) = self
            .copiable_program(source)
            .and_then(Program::permanent)
            .and_then(|perm| perm.entry_tap)
            && entry_tap != crate::land::EntryTap::ChooseCardType
        {
            return None;
        }
        match self
            .characteristics
            .entry_option_choices
            .get(&source)
            .copied()?
        {
            0 => Some(CardType::Artifact),
            1 => Some(CardType::Battle),
            2 => Some(CardType::Creature),
            3 => Some(CardType::Enchantment),
            4 => Some(CardType::Instant),
            5 => Some(CardType::Land),
            6 => Some(CardType::Planeswalker),
            7 => Some(CardType::Sorcery),
            _ => None,
        }
    }

    /// True if `names` matches the card name chosen during resolution or as `source` entered.
    pub(super) fn object_matches_chosen_name(
        &self,
        names: &spellwake_contracts::names::ObjectNames,
        source: Option<ObjectId>,
    ) -> bool {
        if let Some(chosen) = self
            .resolution_cursor()
            .and_then(|cursor| cursor.results.chosen_card_name.as_deref())
            && names.iter().any(|name| name.as_str() == chosen)
        {
            return true;
        }
        let Some(src) = source else {
            return false;
        };
        if let Some(chosen) = self.characteristics.named_cards.get(&src)
            && names.iter().any(|name| name.as_str() == chosen.as_str())
        {
            return true;
        }
        if let Some(crate::land::EntryChoice::CardName(index)) = self.entry_choices.get(&src)
            && let Some(card_name) = self
                .copiable_program(src)
                .and_then(Program::nonland_card_names)
                .and_then(|table| table.get(*index))
        {
            return self.name_rules.has_name(names, card_name);
        }
        false
    }

    fn check_split_second(&self, evaluated: &EvaluatedCharacteristics) -> Result<(), EffectError> {
        let blocks = self
            .window
            .entries()
            .iter()
            .any(|entry| match entry.kind() {
                StackKind::CardSpell(source) => evaluated
                    .get(*source)
                    .is_some_and(|spell| spell.has_keyword(KeywordAbility::SplitSecond)),
                StackKind::SpellCopy { .. } => matches!(self.payloads.get(&entry.id()),
                Some(StackPayload::Instructions(payload)) if payload.copiable_traits.as_ref()
                    .is_some_and(|traits| traits.keywords.contains(&KeywordAbility::SplitSecond))),
                StackKind::Speed
                | StackKind::Monarch
                | StackKind::DelayedCopy { .. }
                | StackKind::Emblem { .. }
                | StackKind::ActivatedAbility { .. }
                | StackKind::TriggeredAbility { .. } => false,
            });
        if blocks {
            Err(EffectError::SplitSecond)
        } else {
            Ok(())
        }
    }

    /// Current prevention of countering applies to both physical spells and spell copies.
    pub(super) fn spell_cannot_be_countered_by(
        &self,
        id: StackEntryId,
        countering: Option<&crate::stack_window::StackEntry>,
    ) -> Result<bool, EffectError> {
        let Some(entry) = self.window.entries().iter().find(|entry| entry.id() == id) else {
            return Ok(false);
        };
        let Some(traits) = self.stack_copiable_traits(id) else {
            return Ok(false);
        };
        if traits.cannot_be_countered
            || entry.protected_by_spent_mana()
            || matches!(
                self.payloads.get(&id),
                Some(StackPayload::Instructions(p)) if p.cannot_be_countered_by_temp
            )
            || self.uncounterable_stack_entries.contains(&id)
        {
            return Ok(true);
        }
        let evaluated = self.evaluated_characteristics()?;
        let counter_colors = countering.map(|c| self.targeting_entry_colors(c, &evaluated));
        if let Some(cols) = counter_colors.as_ref()
            && self
                .uncounterable_by_colors
                .get(&id)
                .is_some_and(|mask| mask.matches_any(cols))
        {
            return Ok(true);
        }
        let types = match entry.kind() {
            StackKind::CardSpell(object) => evaluated
                .get(*object)
                .ok_or(EffectError::InconsistentProgram)?
                .current_types
                .clone(),
            StackKind::SpellCopy { .. } => traits.card_types.clone(),
            _ => return Ok(false),
        };
        let cast_value = match entry.kind() {
            StackKind::CardSpell(object) => Some(self.stack_spell_mana_value(*object)?),
            _ => None,
        };
        if self.temporary_rules.iter().any(|active| {
            if active.consumed {
                return false;
            }
            let (filter, controller_only, next_only) = match &active.rule {
                crate::program::TemporaryRuleKind::CantBeCountered {
                    filter,
                    controller_only,
                    next_only,
                } => (filter, *controller_only, *next_only),
                crate::program::TemporaryRuleKind::CantBeCounteredByColors {
                    colors,
                    filter,
                    controller_only,
                    next_only,
                } => {
                    if !counter_colors
                        .as_ref()
                        .is_some_and(|cols| colors.matches_any(cols))
                    {
                        return false;
                    }
                    (filter, *controller_only, *next_only)
                }
                _ => return false,
            };
            if next_only {
                return false;
            }
            if controller_only && entry.controller() != active.controller {
                return false;
            }
            match filter {
                None => true,
                Some(f) => match entry.kind() {
                    StackKind::CardSpell(obj) => evaluated.get(*obj).is_some_and(|view| {
                        self.matches_object_filter(
                            *f,
                            view,
                            active.controller,
                            None,
                            &BTreeSet::new(),
                            &evaluated,
                        )
                    }),
                    _ => true,
                },
            }
        }) {
            return Ok(true);
        }
        Ok(evaluated.objects.values().any(|source| {
            if source.zone != ObjectZone::Battlefield {
                return false;
            }
            let same_controller = source.controller == Some(entry.controller());
            source
                .printed_text
                .continuous
                .iter()
                .chain(&source.printed_text.additional_battlefield_abilities)
                .any(|ability| {
                    let active = source.abilities.continuous == Some(ability.id())
                        || source
                            .abilities
                            .additional_battlefield_abilities
                            .contains(&ability.id());
                    if !active {
                        return false;
                    }
                    match ability {
                        crate::characteristics::StaticContinuousAbility::CastPermission {
                            permission:
                                crate::characteristics::CastPermissionRule::ControllerSpellsCannotBeCountered(
                                    matching,
                                ),
                            ..
                        } if same_controller => !matching.is_disjoint(&types),
                        crate::characteristics::StaticContinuousAbility::CastPermission {
                            permission:
                                crate::characteristics::CastPermissionRule::ControllerSpellsCannotBeCounteredFiltered(
                                    filter,
                                ),
                            ..
                        } if same_controller => match entry.kind() {
                            StackKind::CardSpell(obj) => {
                                evaluated.get(*obj).is_some_and(|view| {
                                    let aura_or_equip = view.has_type(CardType::Enchantment)
                                        && self
                                            .copiable_program(*obj)
                                            .and_then(Program::permanent)
                                            .is_some_and(|p| {
                                                matches!(
                                                    p.attachment,
                                                    Some(crate::attachment::AttachmentKind::Aura(_))
                                                )
                                            })
                                        || view.has_type(CardType::Artifact)
                                            && self
                                                .copiable_program(*obj)
                                                .and_then(Program::permanent)
                                                .is_some_and(|p| {
                                                    matches!(
                                                        p.attachment,
                                                        Some(
                                                            crate::attachment::AttachmentKind::Equipment
                                                        )
                                                    )
                                                });
                                    filter.matches_characteristics(
                                        view,
                                        aura_or_equip,
                                        source.entry_creature_type,
                                    )
                                })
                            }
                            StackKind::SpellCopy { .. } => filter.matches_card_types(&types),
                            _ => false,
                        },
                        crate::characteristics::StaticContinuousAbility::CastPermission {
                            permission:
                                crate::characteristics::CastPermissionRule::AllSpellsCannotBeCounteredFiltered(
                                    filter,
                                ),
                            ..
                        } => match entry.kind() {
                            StackKind::CardSpell(obj) => {
                                evaluated.get(*obj).is_some_and(|view| {
                                    let aura_or_equip = view.has_type(CardType::Enchantment)
                                        && self
                                            .copiable_program(*obj)
                                            .and_then(Program::permanent)
                                            .is_some_and(|p| {
                                                matches!(
                                                    p.attachment,
                                                    Some(crate::attachment::AttachmentKind::Aura(_))
                                                )
                                            })
                                        || view.has_type(CardType::Artifact)
                                            && self
                                                .copiable_program(*obj)
                                                .and_then(Program::permanent)
                                                .is_some_and(|p| {
                                                    matches!(
                                                        p.attachment,
                                                        Some(
                                                            crate::attachment::AttachmentKind::Equipment
                                                        )
                                                    )
                                                });
                                    filter.matches_characteristics(
                                        view,
                                        aura_or_equip,
                                        source.entry_creature_type,
                                    )
                                })
                            }
                            StackKind::SpellCopy { .. } => filter.matches_card_types(&types),
                            _ => false,
                        },
                        crate::characteristics::StaticContinuousAbility::CostModifier {
                            modifier:
                                crate::characteristics::SpellCostModifier::HighManaValueProtected {
                                    minimum,
                                    ..
                                },
                            ..
                        } if same_controller => cast_value
                            .as_ref()
                            .is_some_and(|value| value >= &i64::from(minimum.get()).into()),
                        _ => false,
                    }
                })
        }))
    }

    fn stack_copiable_traits(&self, entry: StackEntryId) -> Option<CopiableSpellTraitsRef<'_>> {
        let entry = self
            .window
            .entries()
            .iter()
            .chain(self.window.resolving())
            .find(|value| value.id() == entry)?;
        if let Some(source) = self.window.spell_object(entry) {
            return self
                .copiable_program(source)
                .map(|program| CopiableSpellTraitsRef {
                    cannot_be_countered: program.cannot_be_countered,
                    keywords: &program.printed_keywords,
                    names: &program.names,
                    supertypes: &program.supertypes,
                    card_types: &program.card_types,
                    creature_subtypes: &program.creature_subtypes,
                    spell_subtypes: &program.spell_subtypes,
                    artifact_subtypes: &program.artifact_subtypes,
                    enchantment_subtypes: &program.enchantment_subtypes,
                    triggered: &program.triggered,
                });
        }
        match entry.kind() {
            StackKind::CardSpell(_) => None,
            StackKind::SpellCopy { .. } => match self.payloads.get(&entry.id())? {
                StackPayload::Instructions(payload) => {
                    payload
                        .copiable_traits
                        .as_ref()
                        .map(|traits| CopiableSpellTraitsRef {
                            cannot_be_countered: traits.cannot_be_countered,
                            keywords: &traits.keywords,
                            names: &traits.names,
                            supertypes: &traits.supertypes,
                            card_types: &traits.card_types,
                            creature_subtypes: &traits.creature_subtypes,
                            spell_subtypes: &traits.spell_subtypes,
                            artifact_subtypes: &traits.artifact_subtypes,
                            enchantment_subtypes: &traits.enchantment_subtypes,
                            triggered: &traits.triggered,
                        })
                }
                StackPayload::Permanent => None,
            },
            StackKind::Speed
            | StackKind::Monarch
            | StackKind::DelayedCopy { .. }
            | StackKind::Emblem { .. }
            | StackKind::ActivatedAbility { .. }
            | StackKind::TriggeredAbility { .. } => None,
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
        mut waiting: WaitingInstance,
        selected: Option<(ExecutableBody, Vec<modes::SelectedMode>)>,
    ) -> Result<(), EffectError> {
        let actual = self
            .window
            .entries()
            .iter()
            .find(|value| value.id() == entry)
            .ok_or(EffectError::InconsistentProgram)?;
        let has_fixed_clauses = waiting
            .body
            .instructions
            .iter()
            .any(|op| matches!(op, Instruction::FixedTargetClause(_)));
        if actual.controller() != waiting.controller
            || !matches!(actual.kind(), StackKind::TriggeredAbility { source } if source == &waiting.source.object)
            || ((waiting.body.triggered_modes().is_some() || has_fixed_clauses)
                != selected.is_some())
            || (selected.is_none()
                && match waiting.body.target {
                    BodyTarget::None => !actual.targets().targets().is_empty(),
                    _ => {
                        let (minimum, maximum) = waiting
                            .body
                            .permanent_target_bounds()
                            .or_else(|| waiting.body.ordered_any_target_bounds())
                            .unwrap_or((1, 1));
                        !(minimum..=maximum).contains(&actual.targets().targets().len())
                    }
                })
        {
            return Err(EffectError::InconsistentProgram);
        }
        let mut target_controllers = BTreeMap::new();
        if waiting.body.instructions == [Instruction::ExileOnePerPlayerUntilSourceLeaves] {
            let current = self.evaluated_characteristics()?;
            let mut players = BTreeSet::new();
            for bound in actual.targets().targets() {
                let choice = bound.choice();
                let TargetRef::Object(id) = choice.target else {
                    return Err(EffectError::IllegalTarget);
                };
                let controller = current
                    .get(id)
                    .and_then(|view| view.controller)
                    .ok_or(EffectError::IllegalTarget)?;
                if !players.insert(controller) {
                    return Err(EffectError::IllegalTarget);
                }
                target_controllers.insert(choice.id, controller);
            }
        }
        let chosen_modes = if let Some((body, modes)) = selected {
            waiting.body = body;
            modes
        } else {
            Vec::new()
        };
        self.capture_backup(&mut waiting)?;
        let waiting_id = waiting.id;
        let reflexive_context = self.pending_reflexive_context.remove(&waiting_id);
        let (body, trigger) = waiting.into_payload_parts();
        if let Some(x) = trigger
            .causes
            .members()
            .iter()
            .find_map(|member| match &member.related {
                crate::occurrence::TriggerRelated::ReflexiveTrigger { chosen_x } => {
                    chosen_x.clone()
                }
                _ => None,
            })
            .or_else(|| {
                reflexive_context
                    .as_ref()
                    .and_then(|ctx| ctx.resolution_x.clone())
            })
        {
            self.window.set_chosen_x(entry, x)?;
        }
        match self.payloads.entry(entry) {
            std::collections::btree_map::Entry::Vacant(slot) => {
                slot.insert(StackPayload::Instructions(Box::new(InstructionPayload {
                    met_mana_threshold: false,
                    target_controllers,
                    revealed_cost_card: None,
                    sacrificed_permanent_mana_value: None,
                    activated_ability: None,
                    myriad: None,
                    delayed_copy_action: None,
                    activated_as_loyalty: false,
                    activated_planeswalker_types: BTreeSet::new(),
                    spell_target_mana_value: None,
                    copied_mana_value: None,
                    departed_source_counters: BTreeMap::new(),
                    encore_source: None,
                    source_charge_count: None,
                    sacrificed_creature_power: None,
                    sacrificed_creature_toughness: None,
                    exiled_cost_creature: None,
                    exiled_activation_copy: None,
                    granting_damage_source: None,
                    sacrificed_creature_subtypes: BTreeSet::new(),
                    paid_creature_taps: 0,
                    paid_named_counters: None,
                    target_division: BTreeMap::new(),
                    splice_target_bodies: BTreeMap::new(),
                    modes: chosen_modes,
                    rebound_over_buyback: None,
                    exile_on_resolve: false,
                    return_to_hand_on_resolve: false,
                    library_on_resolve: None,
                    cannot_be_countered_by_temp: false,
                    reflexive_context,
                    cascade: None,
                    copy_run: None,
                    pending_copy_targets: Vec::new(),
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
    /// Current-turn event facts for decks with conditional casting costs.
    pub fn cast_turn_history(&self) -> Option<&CastTurnHistory> {
        self.cast_history.as_ref()
    }
    /// Renowned battlefield incarnations (CR 702.112). The designation survives ability and counter removal.
    /// Current age counters by battlefield incarnation, absent at zero.
    pub fn age_counters(&self) -> &BTreeMap<ObjectId, RulesInteger> {
        &self.characteristics.age
    }
    /// The actual previous upkeep cut for each player in games containing echo.
    pub fn upkeep_cuts(&self) -> Option<&BTreeMap<SeatId, crate::object_identity::ControlEpoch>> {
        self.characteristics.upkeep_cuts.as_ref()
    }
    pub fn renowned_objects(&self) -> &BTreeSet<ObjectId> {
        &self.characteristics.renowned
    }

    /// Poison counter totals. An unlisted player has no poison counters.
    pub fn poisoned_players(&self) -> &BTreeMap<SeatId, RulesInteger> {
        &self.poison
    }
    /// A player's current number of poison counters.
    pub fn experience_counters(&self, player: SeatId) -> RulesInteger {
        self.characteristics
            .experience
            .get(&player)
            .cloned()
            .unwrap_or_else(|| 0.into())
    }
    pub fn poison_counters(&self, player: SeatId) -> RulesInteger {
        self.poison
            .get(&player)
            .cloned()
            .unwrap_or_else(|| 0.into())
    }
    pub(crate) fn take_end_turn_jump(&mut self) -> bool {
        std::mem::take(&mut self.ending_turn)
    }
    pub fn life_total(&self, player: SeatId) -> Option<&RulesInteger> {
        self.life.get(player)
    }
    /// Whether an APNAP waiting instance is a delayed sacrifice from a spell copy.
    pub fn waiting_delayed_copy_trigger(
        &self,
        instance: crate::occurrence::TriggerInstanceId,
    ) -> bool {
        self.triggers.delayed_copy_waiting().contains_key(&instance)
    }
    fn delay_spell_return(
        &mut self,
        entry: StackEntryId,
        objects: Vec<ObjectId>,
    ) -> Result<(), EffectError> {
        if objects.is_empty() {
            return Ok(());
        }
        let entry = self
            .window
            .resolving()
            .filter(|resolving| resolving.id() == entry)
            .ok_or(EffectError::WrongResolutionFrame)?;
        match entry.kind() {
            StackKind::CardSpell(id) => {
                let source = self
                    .rules_snapshot(None)?
                    .view()
                    .appearance(*id)
                    .ok_or(EffectError::InconsistentProgram)?;
                self.triggers
                    .delay_exile_return(source, entry.controller(), objects);
            }
            StackKind::SpellCopy { .. } => {
                self.triggers
                    .delay_copy_exile_return(entry.id(), entry.controller(), objects);
            }
            _ => return Err(EffectError::InconsistentProgram),
        }
        Ok(())
    }

    fn delay_spell_return_with_counters(
        &mut self,
        entry: &crate::stack_window::StackEntry,
        objects: Vec<ObjectId>,
    ) -> Result<(), EffectError> {
        match entry.kind() {
            StackKind::CardSpell(id) => {
                let source = self
                    .rules_snapshot(None)?
                    .view()
                    .appearance(*id)
                    .ok_or(EffectError::InconsistentProgram)?;
                self.triggers
                    .delay_return_with_counters(source, entry.controller(), objects);
            }
            StackKind::SpellCopy { .. } => self.triggers.delay_copy_return_with_counters(
                entry.id(),
                entry.controller(),
                objects,
            ),
            _ => return Err(EffectError::InconsistentProgram),
        }
        Ok(())
    }

    /// Capture one next-end-step sacrifice trigger from a resolving physical spell or copy.
    pub(crate) fn delay_spell_sacrifice(
        &mut self,
        entry: StackEntryId,
        controller: SeatId,
        objects: Vec<ObjectId>,
    ) -> Result<(), EffectError> {
        let actual = self
            .window
            .resolving()
            .filter(|actual| actual.id() == entry && actual.controller() == controller)
            .ok_or(EffectError::WrongResolutionFrame)?;
        if objects.iter().copied().collect::<BTreeSet<_>>().len() != objects.len()
            || objects.iter().any(|id| {
                self.window
                    .cards()
                    .get(*id)
                    .is_none_or(|card| card.zone() != ObjectZone::Battlefield)
            })
        {
            return Err(EffectError::InconsistentProgram);
        }
        match actual.kind() {
            StackKind::CardSpell(id) => {
                let source = self
                    .rules_snapshot(None)?
                    .view()
                    .appearance(*id)
                    .ok_or(EffectError::InconsistentProgram)?;
                self.triggers
                    .delay_sacrifice(source, controller, crate::turn::Step::End, objects);
            }
            StackKind::ActivatedAbility { source } | StackKind::TriggeredAbility { source } => {
                let appearance = self
                    .rules_snapshot(None)?
                    .view()
                    .appearance(source.id())
                    .or_else(|| {
                        self.execution_bindings(entry)
                            .and_then(|b| b.trigger.as_ref())
                            .map(|t| t.source.clone())
                    })
                    .ok_or(EffectError::InconsistentProgram)?;
                self.triggers.delay_sacrifice(
                    appearance,
                    controller,
                    crate::turn::Step::End,
                    objects,
                );
            }
            StackKind::SpellCopy { .. } => self.triggers.delay_copy_sacrifice(
                entry,
                controller,
                crate::turn::Step::End,
                objects,
            ),
            _ => return Err(EffectError::InconsistentProgram),
        }
        Ok(())
    }

    /// Capture one delayed exile trigger for newly created tokens from any resolving stack entry.
    pub(crate) fn delay_entry_exile(
        &mut self,
        entry: StackEntryId,
        controller: SeatId,
        step: crate::turn::Step,
        objects: Vec<ObjectId>,
    ) -> Result<(), EffectError> {
        let actual = self
            .window
            .resolving()
            .filter(|actual| actual.id() == entry && actual.controller() == controller)
            .ok_or(EffectError::WrongResolutionFrame)?;
        if objects.iter().copied().collect::<BTreeSet<_>>().len() != objects.len()
            || objects.iter().any(|id| {
                self.window
                    .cards()
                    .get(*id)
                    .is_none_or(|card| card.zone() != ObjectZone::Battlefield)
            })
        {
            return Err(EffectError::InconsistentProgram);
        }
        match actual.kind() {
            StackKind::CardSpell(id) => {
                let source = self
                    .rules_snapshot(None)?
                    .view()
                    .appearance(*id)
                    .ok_or(EffectError::InconsistentProgram)?;
                self.triggers.delay_exile(source, controller, step, objects);
            }
            StackKind::ActivatedAbility { source } | StackKind::TriggeredAbility { source } => {
                let appearance = self
                    .execution_bindings(entry)
                    .and_then(|b| b.trigger.as_ref())
                    .map(|t| t.source.clone())
                    .or_else(|| {
                        self.rules_snapshot(None)
                            .ok()
                            .and_then(|s| s.view().appearance(source.id()))
                    })
                    .ok_or(EffectError::InconsistentProgram)?;
                self.triggers
                    .delay_exile(appearance, controller, step, objects);
            }
            StackKind::SpellCopy { .. } => {
                self.triggers
                    .delay_copy_exile(entry, controller, step, objects);
            }
            _ => return Err(EffectError::InconsistentProgram),
        }
        Ok(())
    }
    /// The player entitled to inspect this exact face-down exile incarnation, if any.
    pub fn face_down_exile_viewer(&self, object: ObjectId) -> Option<SeatId> {
        if !self
            .window
            .cards()
            .get(object)
            .is_some_and(|card| card.zone() == ObjectZone::Exile)
        {
            return None;
        }
        if let Some(&viewer) = self.face_down_exile.get(&object) {
            return Some(viewer);
        }
        self.evaluated_characteristics().ok().and_then(|evaluated| {
            evaluated.objects.values().find_map(|source| {
                let linked = self
                    .linked_exiles
                    .get(&source.object)
                    .is_some_and(|ids| ids.contains(&object))
                    || source.imprinted_cards.contains(&object);
                (source.zone == ObjectZone::Battlefield
                    && linked
                    && source.has_battlefield_rule(|rule| {
                        matches!(
                            rule,
                            crate::characteristics::StaticContinuousAbility::CastPermission {
                                permission:
                                    crate::characteristics::CastPermissionRule::LookAtSourceExiledCards,
                                ..
                            }
                        )
                    }))
                    .then_some(source.controller)
                    .flatten()
            })
        })
    }

    /// Mana spending flexibility granted to this player for the exact exiled card.
    /// The returned flag additionally permits paying colorless requirements with colored mana.
    pub fn flexible_exile_mana(&self, object: ObjectId, player: SeatId) -> Option<bool> {
        if self.window.cards().get(object)?.zone() != ObjectZone::Exile {
            return None;
        }
        if self.any_type_exile_cast.contains(&(object, player)) {
            Some(true)
        } else {
            (self.card_play_permissions.iter().any(|grant| {
                grant.zone == CardPlayZone::Exile
                    && grant.object == object
                    && grant.player == player
                    && grant.any_color
            }) || self
                .temporary_any_color_exile_cast
                .contains(&(object, player)))
            .then_some(false)
        }
    }
    /// Whether this exact card still has the temporary flashback permission.
    /// Timing, chosen cost and target legality are checked during announcement.
    pub fn has_granted_flashback(&self, object: ObjectId, player: SeatId) -> bool {
        self.granted_flashback.contains(&object)
            && self
                .window
                .cards()
                .get(object)
                .is_some_and(|card| card.zone() == ObjectZone::Graveyard(player))
    }

    /// Whether this exact graveyard card has an ordinary-price cast permission for the player.
    pub fn may_cast_from_graveyard_by_permission(&self, object: ObjectId, player: SeatId) -> bool {
        self.window
            .cards()
            .get(object)
            .is_some_and(|card| CardPlayZone::Graveyard.allows(card.zone()))
            && self.card_play_permissions.iter().any(|grant| {
                grant.zone == CardPlayZone::Graveyard
                    && grant.object == object
                    && grant.player == player
            })
    }

    /// Check active `CardPlayPermission` (non-exile) and `TemporaryRuleKind::ZoneCastPermission`
    /// grants for casting `object`. Returns `(zone_allowed, free, any_color, consumed_rule_idx)`.
    pub(super) fn temporary_zone_cast_grant(
        &self,
        object: ObjectId,
        player: SeatId,
        current: &EvaluatedCharacteristics,
        face: Option<&Program>,
        turn: Option<&TurnScheduler>,
    ) -> (bool, bool, bool, Option<usize>) {
        let Some(card) = self.window.cards().get(object) else {
            return (false, false, false, None);
        };
        let is_land = face.is_some_and(|p| p.has_type(CardType::Land));
        let mut zone_allowed = false;
        let mut free = false;
        let mut any_color = false;
        for grant in &self.card_play_permissions {
            if grant.object == object
                && grant.player == player
                && grant.zone.allows(card.zone())
                && !(grant.cast_only && is_land)
            {
                zone_allowed = true;
                free |= grant.free && !is_land;
                any_color |= grant.any_color;
            }
        }
        let mut consumed_idx = None;
        if let Some(view) = current.get(object) {
            for (idx, active) in self.temporary_rules.iter().enumerate() {
                if active.consumed || !active.resolved_players.contains(&player) {
                    continue;
                }
                let crate::program::TemporaryRuleKind::ZoneCastPermission {
                    zone,
                    filter,
                    cast_only,
                    free: rule_free,
                    any_color: rule_any_color,
                    once,
                    sorcery_only,
                    ..
                } = &active.rule
                else {
                    continue;
                };
                if *cast_only && is_land {
                    continue;
                }
                if *sorcery_only && validate_main_phase(&self.window, turn, player).is_err() {
                    continue;
                }
                let zone_ok = match zone {
                    crate::program::SourceZoneFilter::Hand => {
                        card.zone() == ObjectZone::Hand(player)
                    }
                    crate::program::SourceZoneFilter::Graveyard => {
                        card.zone() == ObjectZone::Graveyard(player)
                    }
                    crate::program::SourceZoneFilter::HandOrGraveyard => {
                        card.zone() == ObjectZone::Hand(player)
                            || card.zone() == ObjectZone::Graveyard(player)
                    }
                    crate::program::SourceZoneFilter::OpponentGraveyard => {
                        matches!(card.zone(), ObjectZone::Graveyard(owner) if owner != player)
                    }
                    crate::program::SourceZoneFilter::AnyGraveyard => {
                        matches!(card.zone(), ObjectZone::Graveyard(_))
                    }
                    crate::program::SourceZoneFilter::Exile => card.zone() == ObjectZone::Exile,
                    crate::program::SourceZoneFilter::Battlefield => {
                        card.zone() == ObjectZone::Battlefield
                    }
                    crate::program::SourceZoneFilter::YourLibrary => {
                        card.zone() == ObjectZone::Library(player)
                    }
                    crate::program::SourceZoneFilter::OpponentExile => {
                        card.zone() == ObjectZone::Exile && card.owner() != player
                    }
                    crate::program::SourceZoneFilter::LinkedExile => {
                        card.zone() == ObjectZone::Exile
                            && active.source_object.is_some_and(|src| {
                                self.linked_exiles
                                    .get(&src)
                                    .is_some_and(|list| list.contains(&object))
                                    || self
                                        .characteristics
                                        .imprinted_cards
                                        .get(&src)
                                        .is_some_and(|list| list.contains(&object))
                            })
                    }
                    crate::program::SourceZoneFilter::LastMoved => false,
                };
                if !zone_ok {
                    continue;
                }
                if filter.is_some_and(|f| {
                    !self.matches_object_filter(
                        f,
                        view,
                        active.controller,
                        None,
                        &BTreeSet::new(),
                        current,
                    )
                }) {
                    continue;
                }
                zone_allowed = true;
                free |= *rule_free && !is_land;
                any_color |= *rule_any_color;
                if *once && consumed_idx.is_none() {
                    consumed_idx = Some(idx);
                }
            }
        }
        (zone_allowed, free, any_color, consumed_idx)
    }

    /// An unexpired free-cast permission constrained by its granting effect.
    pub fn may_cast_free_from_exile(&self, object: ObjectId, player: SeatId) -> bool {
        self.may_cast_free_face(object, player, self.copiable_program(object))
    }
    fn may_cast_free_face(&self, object: ObjectId, player: SeatId, face: Option<&Program>) -> bool {
        let Some(program) = face.filter(|p| !p.has_type(CardType::Land)) else {
            return false;
        };
        self.window
            .cards()
            .get(object)
            .is_some_and(|card| card.zone() == ObjectZone::Exile)
            && (self.free_exiled_cards.contains(&(object, player))
                || self.card_play_permissions.iter().any(|grant| {
                    grant.zone == CardPlayZone::Exile
                        && grant.object == object
                        && grant.player == player
                        && grant.free
                })
                || !self
                    .free_exile_types_for(object, player, Some(program))
                    .is_empty()
                || self
                    .free_exiled_noncreature_spells
                    .contains(&(object, player))
                    && !program.has_type(CardType::Creature))
    }
    /// Unused card types that can pay for an exact exiled spell under its granting effect.
    pub fn free_exile_types(&self, object: ObjectId, player: SeatId) -> Vec<CardType> {
        self.free_exile_types_for(object, player, self.copiable_program(object))
    }
    fn free_exile_types_for(
        &self,
        object: ObjectId,
        player: SeatId,
        face: Option<&Program>,
    ) -> Vec<CardType> {
        if self
            .window
            .cards()
            .get(object)
            .is_none_or(|card| card.zone() != ObjectZone::Exile)
        {
            return Vec::new();
        }
        let Some(program) = face.filter(|p| !p.has_type(CardType::Land)) else {
            return Vec::new();
        };
        let Some(cohort) = self
            .typed_exile_cohorts
            .values()
            .find(|c| c.player == player && c.cards.contains(&object))
        else {
            return Vec::new();
        };
        program
            .card_types
            .iter()
            .copied()
            .filter(|ty| !cohort.used.contains(ty))
            .collect()
    }
    /// Cast permission for an exact card exiled face down by this player.
    pub fn may_cast_from_face_down_exile(&self, object: ObjectId, player: SeatId) -> bool {
        self.face_down_exile_viewer(object) == Some(player)
            && self.may_cast_from_granted_exile(object, player)
    }
    /// Permission to cast a specific exiled spell with flexible mana.
    pub fn may_cast_from_granted_exile(&self, object: ObjectId, player: SeatId) -> bool {
        (self.any_type_exile_cast.contains(&(object, player))
            || self
                .temporary_any_color_exile_cast
                .contains(&(object, player)))
            && self
                .window
                .cards()
                .get(object)
                .is_some_and(|card| card.zone() == ObjectZone::Exile)
            && self
                .copiable_program(object)
                .is_some_and(|program| !program.has_type(CardType::Land))
    }
    /// Whether an exact exiled incarnation has an active play permission.
    pub fn may_play_from_exile(&self, object: ObjectId, player: SeatId) -> bool {
        self.play_exile_permission(object, player, true)
    }
    fn play_exile_permission(&self, object: ObjectId, player: SeatId, normal_face: bool) -> bool {
        let Some(card) = self
            .window
            .cards()
            .get(object)
            .filter(|card| card.zone() == ObjectZone::Exile && card.card_id().is_some())
        else {
            return false;
        };
        (normal_face
            && card.owner() == player
            && card.face_down().is_none()
            && self
                .copiable_program(object)
                .is_some_and(|program| program.cast_from_own_exile))
            || (normal_face && self.adventured.contains(&(object, player))
                || self.card_play_permissions.iter().any(|grant| {
                    grant.zone == CardPlayZone::Exile
                        && grant.object == object
                        && grant.player == player
                })
                || self
                    .exile_play_until_next
                    .values()
                    .any(|granted| *granted == (object, player)))
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
    /// Printed definition of the other physical face, if this is a double-faced card.
    pub fn opposite_physical_face(&self, id: ObjectId) -> Option<&Program> {
        let card = self.window.cards().get(id)?;
        let front = self.programs.get(&card.card_id()?)?;
        let back = front.back_face()?;
        Some(if card.transformed() { front } else { back })
    }
    /// A transforming card's back face uses its front's mana value, except when copying another permanent.
    /// Modal double-faced back faces use their own face's mana value (CR 712.8f).
    pub fn permanent_mana_value(&self, id: ObjectId) -> Option<RulesInteger> {
        let permanent = self.window.cards().get(id)?;
        if permanent.zone() != ObjectZone::Battlefield {
            return None;
        }
        if permanent.face_down().is_some() {
            return Some(0.into());
        }
        if self.characteristics.copied_definition(id).is_none()
            && let Some(value) = permanent.melded_mana_value(&self.programs)
        {
            return Some(value);
        }
        if permanent.transformed()
            && self.characteristics.copied_definition(id).is_none()
            && let Some(front) = self.programs.get(&permanent.card_id()?)
            && front.modal_back.is_none()
        {
            return Some(front.mana_value());
        }
        self.copiable_program(id).map(Program::mana_value)
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

    /// Visible source face for a known object, with later copy and merge effects
    /// applied in timestamp order. The caller authorizes visibility independently.
    pub fn display_program(&self, object: ObjectId) -> Option<&Program> {
        let physical = self.window.cards().get(object)?;
        if physical.zone() == ObjectZone::Battlefield
            && physical.face_down().is_none()
            && let Some(face) = self.characteristics.displayed_copy_face(object)
        {
            return Some(face);
        }
        Some(physical.displayed_face(self.copiable_program(object)?))
    }

    /// The two public participant choices made by this permanent’s controller as it entered.
    pub fn chosen_entry_players(&self, source: ObjectId) -> Option<[SeatId; 2]> {
        self.characteristics
            .chosen_entry_players
            .get(&source)
            .copied()
    }

    fn valid_distinct_players(&self, [first, second]: [SeatId; 2]) -> bool {
        first != second
            && self.window.participants().contains(&first)
            && self.window.participants().contains(&second)
    }
    /// Physical face information for the status display of phased-out permanents.
    /// The caller applies viewer permissions to face-down objects before presenting names.
    pub fn phased_permanents(&self) -> impl Iterator<Item = (&GameObject, SeatId, &Program)> {
        let cards = self.window.cards();
        cards.physical_objects().filter_map(move |object| {
            Some((
                object,
                cards.phased_out_under(object.id())?,
                self.characteristics
                    .copiable_definition(object, &self.programs)?
                    .0,
            ))
        })
    }
    /// Printed face for a present or publicly revealed historical incarnation.
    pub fn printed_face<'a>(&'a self, object: &'a GameObject) -> Option<&'a Program> {
        object.base_program(&self.programs)
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
        self.evaluate_for_participants(self.window.participants())
    }

    /// Snapshot CR 608.2h last-known information for a targeted object before or during resolution.
    pub(super) fn capture_target_lki(
        &self,
        id: ObjectId,
        evaluated: &EvaluatedCharacteristics,
    ) -> Option<TargetLki> {
        let object = self.window.cards().get(id)?;
        let view = evaluated.get(id);
        let controller = view
            .and_then(|v| v.controller)
            .or_else(|| object.controller());
        let mana_value = self
            .copiable_program(id)
            .map(Program::mana_value)
            .unwrap_or_else(|| 0.into());
        let (power, toughness) = match view.and_then(|v| v.power_toughness.as_ref()) {
            Some(pt) => (
                Some(pt.derived.power.clone()),
                Some(pt.derived.toughness.clone()),
            ),
            None => (None, None),
        };
        Some(TargetLki {
            owner: object.owner(),
            controller,
            mana_value,
            power,
            toughness,
        })
    }

    fn evaluate_for_participants(
        &self,
        participants: &[SeatId],
    ) -> Result<EvaluatedCharacteristics, EffectError> {
        let mut evaluated = evaluate(CharacteristicWorld {
            active_player: self.window.active_player(),
            step: self.current_step,
            participants,
            life: Some(&self.life),
            entry_creature_types: &self.entry_creature_type_choices,
            mana: &self.mana,
            combat: self.combat.as_ref(),
            cards: self.window.cards(),
            programs: &self.programs,
            state: &self.characteristics,
            vocabulary: &self.creature_vocabulary,
            name_rules: &self.name_rules,
        })?;
        for active in &self.temporary_rules {
            if active.consumed {
                continue;
            }
            if let crate::program::TemporaryRuleKind::PlayerGrantKeyword { keywords, .. } =
                &active.rule
            {
                for &player in &active.resolved_players {
                    evaluated
                        .player_granted_keywords
                        .entry(player)
                        .or_default()
                        .extend(keywords.iter().copied());
                }
            }
        }
        Ok(evaluated)
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
    /// Whether this exact printed or copied ability instance spent its exhaust activation.
    pub fn ability_exhausted(&self, source: ObjectId, ability: ActivatedAbilityId) -> bool {
        let stamp = self
            .characteristics
            .copied_definition(source)
            .map(|(_, stamp)| stamp);
        self.characteristics.exhausted.contains(&(
            source,
            stamp,
            ability,
            crate::characteristics::ActivationOriginPath::direct(ability, None),
        ))
    }
    /// Whether this inherited instance on the recipient spent its exhaust activation.
    pub fn inherited_ability_exhausted(
        &self,
        source: ObjectId,
        terms: &crate::characteristics::ActivationTerms,
    ) -> bool {
        let stamp = self
            .characteristics
            .copied_definition(source)
            .map(|(_, stamp)| stamp);
        self.characteristics.exhausted.contains(&(
            source,
            stamp,
            ActivatedAbilityId(0),
            terms.origin.clone(),
        ))
    }
    /// Physical counters, independent of derived type or power/toughness.
    pub fn power_toughness_counters(&self, object: ObjectId) -> Option<PowerToughnessCounters> {
        self.window
            .cards()
            .get(object)
            .filter(|card| card.zone() == ObjectZone::Battlefield)?;
        Some(self.characteristics.counters(object))
    }
    /// Creatures dealt positive damage with deathtouch since the last SBA check.
    pub fn deathtouch_damage(&self) -> &BTreeSet<ObjectId> {
        &self.deathtouch_damage
    }
    /// Unused regeneration shields on exact permanent incarnations through this turn's cleanup.
    pub fn regeneration_shields(&self) -> &BTreeMap<ObjectId, BTreeSet<(StackEntryId, usize)>> {
        &self.regeneration_shields
    }
    /// Last battlefield appearances still needed by pending damage abilities.
    pub fn departed_damage_sources(
        &self,
    ) -> &BTreeMap<ObjectId, crate::damage::DepartedDamageSource> {
        &self.departed_damage_sources
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
        let (next, outcome, append) = self
            .progress_direct_draw(expected, player, amount, origin)?
            .into_parts();
        Ok(RulesTransition::new(
            next,
            outcome.map_err(|frame| frame.required_choice_error())?,
            append,
        ))
    }
    pub(crate) fn progress_direct_draw(
        &self,
        expected: u64,
        player: SeatId,
        amount: RulesInteger,
        origin: DrawOrigin,
    ) -> Result<RulesTransition<Self, Result<DrawJournal, EffectDecision>>, EffectError> {
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
        let journal = next.draw_with_replacements(player, amount, origin, &mut append)?;
        if journal.is_ok() {
            next.draw_replacement = None;
        }
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
            let retained = self.retained_mana_types(&self.evaluated_characteristics()?);
            next.expire_step_mana(&retained);
        }
        Ok(RulesTransition::new(next, (), RulesJournal::default()))
    }
    /// Actual controller-turn start, separately from untapping. The root calls
    /// this only at an established turn start, including the opening handoff.
    /// Synchronize the scheduler-owned step for effects that offer a cast while resolving.
    pub(crate) fn set_calendar_position(&mut self, position: crate::turn::TurnPosition) {
        self.current_step = Some(position.step);
        self.current_turn_number = position.number;
    }

    /// Metadata-only bookkeeping; this operation emits no gameplay append.
    pub(crate) fn record_turn_start(&self, expected: u64) -> Result<Self, EffectError> {
        let revision = self.next_revision(expected)?;
        if self.pending_work.is_some() {
            return Err(EffectError::WrongPhase);
        }
        if !self.state_based_losses.is_empty() {
            return Err(EffectError::PlayerLossPending);
        }
        let mut next = Self {
            revision,
            window: self.window.record_turn_start(self.window.revision())?,
            ..self.clone()
        };
        let active = next.window.active_player();
        next.attack_directions.retain(|(controller, _)| {
            *controller != active && !next.skipped_turns.contains(controller)
        });
        next.triggers
            .expire_attack_triggers(active, &next.skipped_turns);
        next.triggers.reset_once_each_turn();
        next.required_planeswalker_attacks = next
            .next_turn_attack_targets
            .remove(&active)
            .unwrap_or_default();
        next.next_turn_attack_targets
            .retain(|player, _| !next.skipped_turns.contains(player));
        next.characteristics.loyalty_used.clear();
        next.characteristics.loyalty_activators.clear();
        next.characteristics.extra_loyalty_activations.clear();
        next.characteristics.extra_loyalty_used.clear();
        next.cards_drawn_this_turn.clear();
        next.encore_attack_targets.clear();
        next.characteristics.life_lost_this_turn.clear();
        next.characteristics.life_gained_this_turn.clear();
        next.characteristics.spells_cast_this_turn.clear();
        next.characteristics.attacked_this_turn_players.clear();
        next.characteristics.creatures_entered_this_turn.clear();
        next.characteristics
            .permanents_left_battlefield_this_turn
            .clear();
        next.characteristics.cards_discarded_this_turn.clear();
        next.characteristics.permanents_sacrificed_this_turn.clear();
        next.characteristics.cards_drawn_this_turn_counts.clear();
        next.characteristics.descended_this_turn.clear();
        next.characteristics.committed_crime_this_turn.clear();
        next.characteristics.entered_types_this_turn.clear();
        next.characteristics.permanents_entered_this_turn.clear();
        next.characteristics.cast_spell_types_this_turn.clear();
        next.characteristics
            .noncreature_spells_cast_this_turn
            .clear();
        next.characteristics.spells_cast_from_hand_this_turn.clear();
        next.characteristics.lands_played_this_turn.clear();
        next.characteristics.cards_left_graveyard_this_turn.clear();
        next.characteristics
            .nonland_permanents_left_battlefield_this_turn
            .clear();
        next.characteristics.creatures_died_this_turn_count = 0;
        next.characteristics
            .controlled_creatures_died_this_turn
            .clear();
        next.characteristics.tokens_created_this_turn.clear();
        next.characteristics.attacking_creatures_this_turn.clear();
        next.characteristics.attacked_objects_this_turn.clear();
        next.characteristics.exerted_objects_this_turn.clear();
        next.characteristics.blocked_objects_this_turn.clear();
        next.characteristics.entered_objects_this_turn.clear();
        next.characteristics.died_this_turn.clear();
        next.characteristics.activated_sources_this_turn.clear();
        next.characteristics
            .ability_resolutions_by_source_this_turn
            .clear();
        next.characteristics.players_dealt_damage_this_turn.clear();
        next.characteristics
            .players_dealt_noncombat_damage_this_turn
            .clear();
        next.characteristics.object_damage_taken_this_turn.clear();
        if let Some(pairs) = &mut next.characteristics.creature_damage_pairs {
            pairs.clear();
        }
        next.characteristics.clear_extra_turn_history();
        if let Some(ordinals) = next.spell_ordinals.as_mut() {
            ordinals.clear();
        }
        if let Some(positions) = next.spell_cast_positions.as_mut() {
            *positions = SpellCastPositions::default();
        }
        next.creatures_died_this_turn = 0.into();
        next.additional_combats = 0;
        if let Some(defenders) = next.departed_attack_defenders.as_mut() {
            defenders.clear();
        }
        next.token_creators_this_turn.clear();
        next.free_exiled_cards.clear();
        next.free_exiled_noncreature_spells.clear();
        next.typed_exile_cohorts.clear();
        let active_player = next.window.active_player();
        let mut updated_permissions = BTreeSet::new();
        for mut grant in std::mem::take(&mut next.card_play_permissions) {
            match grant.duration {
                CardPlayDuration::WhileInZone => {
                    updated_permissions.insert(grant);
                }
                CardPlayDuration::ThroughNextOwnTurnSkipCurrent(owner) => {
                    grant.duration = CardPlayDuration::ThroughNextOwnTurn(owner);
                    updated_permissions.insert(grant);
                }
                CardPlayDuration::ThroughNextOwnTurn(owner) if active_player != owner => {
                    updated_permissions.insert(grant);
                }
                CardPlayDuration::ThroughTurn | CardPlayDuration::ThroughNextOwnTurn(_) => {}
            }
        }
        next.card_play_permissions = updated_permissions;
        next.temporary_any_color_exile_cast.clear();
        Ok(next)
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
            && !current.activate_as_though_haste
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
        prepare_payment(cost, selected, |mana_type| {
            self.unrestricted_mana(actor, mana_type)
        })
        .map_err(|error| match error {
            ManaPaymentError::InsufficientMana => EffectError::InsufficientMana,
            ManaPaymentError::PaymentRequired => EffectError::ManaPaymentRequired,
            other => EffectError::InvalidManaPayment(other),
        })
    }
    /// Mana of this type currently available for an ordinary resolution payment.
    pub fn unrestricted_mana(&self, actor: SeatId, mana_type: ManaType) -> RulesInteger {
        let restricted = self
            .restricted_mana
            .iter()
            .filter(|((player, color, _), _)| *player == actor && *color == mana_type)
            .fold(RulesInteger::from(0), |sum, (_, amount)| sum.plus(amount));
        self.mana(actor, mana_type).minus(&restricted)
    }
    fn prepare_cast_mana_payment(
        &self,
        cost: &ManaCost,
        selected: Option<&ManaPayment>,
        actor: SeatId,
        entry: StackEntryId,
        restricted: &[crate::mana::RestrictedManaSpend],
    ) -> Result<Vec<ManaSpend>, EffectError> {
        let spell = self
            .window
            .entries()
            .iter()
            .find(|candidate| candidate.id() == entry)
            .ok_or(EffectError::WrongResolutionFrame)?;
        let current = self.evaluated_characteristics()?;
        let spell_view = match spell.kind() {
            StackKind::CardSpell(object) => current.get(*object),
            _ => None,
        };
        self.prepare_restricted_mana_payment(cost, selected, actor, restricted, |restriction| match restriction {
                    crate::mana::ManaRestriction::PlaneswalkerSpell => spell_view.is_some_and(|value| value.has_type(CardType::Planeswalker)),
                    crate::mana::ManaRestriction::ColorlessSpell => spell_view.is_some_and(|value| value.current_colors.is_empty()),
                    crate::mana::ManaRestriction::SpellWithFlashbackFromGraveyard => {
                        spell.cast_from_graveyard && (spell.cast_with_flashback()
                            || matches!(spell.kind(), StackKind::CardSpell(object)
                                if self.copiable_program(*object).is_some_and(|program| program.flashback.is_some())))
                    }
                    crate::mana::ManaRestriction::NoncreatureSpell => spell_view.is_some_and(|value| !value.has_type(CardType::Creature)),
                    crate::mana::ManaRestriction::ArtifactSpellOrAbility
                    | crate::mana::ManaRestriction::NotNonartifactSpell => spell_view.is_some_and(|value| value.has_type(CardType::Artifact)),
                    crate::mana::ManaRestriction::CommanderSpell => match spell.kind() {
                        StackKind::CardSpell(object) => self.window.cards().get(*object).is_some_and(|card| card.owner() == actor && card.card_id().is_some_and(|id| self.commanders.contains_key(&id))),
                        _ => false,
                    },
                    crate::mana::ManaRestriction::ColorlessEldraziSpellOrAbility => self.creature_vocabulary.resolve_label("Eldrazi").is_some_and(|kind| spell_view.is_some_and(|value| value.current_colors.is_empty() && value.current_creature_subtypes.contains(&kind))),
                    crate::mana::ManaRestriction::CreatureSpell => spell_view.is_some_and(|value| value.has_type(CardType::Creature)),
                    crate::mana::ManaRestriction::CreatureSpellOfType(kind)
                    | crate::mana::ManaRestriction::CreatureSpellOfTypeUncounterable(kind)
                    | crate::mana::ManaRestriction::CreatureSpellOrAbilityOfType(kind) => kind.is_some_and(|kind| spell_view.is_some_and(|value| value.has_type(CardType::Creature) && value.current_creature_subtypes.contains(&kind))),
                    crate::mana::ManaRestriction::InstantOrSorcerySpell
                    | crate::mana::ManaRestriction::InstantOrSorceryOrForetell => spell_view.is_some_and(|value| value.has_type(CardType::Instant) || value.has_type(CardType::Sorcery)),
                    crate::mana::ManaRestriction::ArtifactSpell
                    | crate::mana::ManaRestriction::ArtifactSpellOrAnyAbility => spell_view.is_some_and(|value| value.has_type(CardType::Artifact)),
                    crate::mana::ManaRestriction::ArtifactAbility
                    | crate::mana::ManaRestriction::ActivatedAbility => false,
                    crate::mana::ManaRestriction::CastSpell => true,
                    crate::mana::ManaRestriction::NotSpellFromHand => !spell.cast_from_hand_of(actor),
                    crate::mana::ManaRestriction::CostContainingX => matches!(spell.kind(), StackKind::CardSpell(object) if self.copiable_program(*object).is_some_and(|p| p.x_mana_symbols > 0)),
                    crate::mana::ManaRestriction::LegendarySpell => spell_view.is_some_and(|value| value.current_supertypes.contains(&crate::program::Supertype::Legendary)),
                    crate::mana::ManaRestriction::MulticoloredSpell => spell_view.is_some_and(|value| value.current_colors.len() >= 2),
                    crate::mana::ManaRestriction::EnchantmentSpell => spell_view.is_some_and(|value| value.has_type(CardType::Enchantment)),
                    crate::mana::ManaRestriction::CreatureSpellOrAbility => spell_view.is_some_and(|value| value.has_type(CardType::Creature)),
                    crate::mana::ManaRestriction::ColoredSpellWithoutX => spell_view.is_some_and(|value| !value.current_colors.is_empty()) && matches!(spell.kind(), StackKind::CardSpell(object) if self.copiable_program(*object).is_some_and(|p| p.x_mana_symbols == 0)),
                    crate::mana::ManaRestriction::SpellWithMinManaValue(min) => match spell.kind() {
                        StackKind::CardSpell(object) => self.stack_spell_mana_value(*object).is_ok_and(|mv| mv >= i64::from(min).into()),
                        _ => false,
                    },
                    crate::mana::ManaRestriction::CreatureSpellWithMinManaValue(min) => {
                        spell_view.is_some_and(|value| value.has_type(CardType::Creature))
                            && matches!(spell.kind(), StackKind::CardSpell(object) if self.stack_spell_mana_value(*object).is_ok_and(|mv| mv >= i64::from(min).into()))
                    }
                    crate::mana::ManaRestriction::HistoricSpell => spell_view.is_some_and(|value| {
                        value.has_type(CardType::Artifact)
                            || value.current_supertypes.contains(&crate::program::Supertype::Legendary)
                            || value.current_enchantment_subtypes.contains(&crate::program::EnchantmentSubtype::Saga)
                    }),
                    crate::mana::ManaRestriction::CreatureOrPlaneswalkerSpell => spell_view.is_some_and(|value| {
                        value.has_type(CardType::Creature) || value.has_type(CardType::Planeswalker)
                    }),
                    crate::mana::ManaRestriction::CreatureAbility => false,
                    crate::mana::ManaRestriction::PermanentSpell => spell_view.is_some_and(|value| {
                        value.has_type(CardType::Artifact)
                            || value.has_type(CardType::Battle)
                            || value.has_type(CardType::Creature)
                            || value.has_type(CardType::Enchantment)
                            || value.has_type(CardType::Planeswalker)
                    }),
                    crate::mana::ManaRestriction::SpellFromGraveyardOrExile => {
                        spell.cast_summary().cast_from_graveyard || spell.cast_summary().cast_from_exile
                    }
                    crate::mana::ManaRestriction::EquipmentOrEquipAbility => spell_view.is_some_and(|value| {
                        value.current_artifact_subtypes.contains(&crate::program::ArtifactSubtype::Equipment)
                    }),
                    crate::mana::ManaRestriction::SpellWithMinManaValueOrX(min) => match spell.kind() {
                        StackKind::CardSpell(object) => {
                            self.stack_spell_mana_value(*object).is_ok_and(|mv| mv >= i64::from(min).into())
                                || self.copiable_program(*object).is_some_and(|p| p.x_mana_symbols > 0)
                        }
                        _ => false,
                    },
                    crate::mana::ManaRestriction::CreatureSpellWithMinManaValueOrX(min) => {
                        spell_view.is_some_and(|value| value.has_type(CardType::Creature))
                            && matches!(spell.kind(), StackKind::CardSpell(object) if self.stack_spell_mana_value(*object).is_ok_and(|mv| mv >= i64::from(min).into()) || self.copiable_program(*object).is_some_and(|p| p.x_mana_symbols > 0))
                    }
                    crate::mana::ManaRestriction::CreatureOrEnchantmentSpell => spell_view.is_some_and(|value| {
                        value.has_type(CardType::Creature) || value.has_type(CardType::Enchantment)
                    }),
        })
    }
    fn prepare_restricted_mana_payment(
        &self,
        cost: &ManaCost,
        selected: Option<&ManaPayment>,
        actor: SeatId,
        restricted: &[crate::mana::RestrictedManaSpend],
        permits: impl Fn(crate::mana::ManaRestriction) -> bool,
    ) -> Result<Vec<ManaSpend>, EffectError> {
        let mut chosen = BTreeMap::new();
        for item in restricted {
            if !item.amount.is_positive()
                || chosen
                    .insert((item.mana_type, item.restriction), item.amount.clone())
                    .is_some()
                || self
                    .restricted_mana
                    .get(&(actor, item.mana_type, item.restriction))
                    .is_none_or(|available| available < &item.amount)
                || !permits(item.restriction)
            {
                return Err(EffectError::InvalidManaPayment(
                    ManaPaymentError::MissingRequired,
                ));
            }
        }
        let spend = prepare_payment(cost, selected, |kind| {
            chosen
                .iter()
                .filter(|((color, _), _)| *color == kind)
                .fold(self.unrestricted_mana(actor, kind), |sum, (_, amount)| {
                    sum.plus(amount)
                })
        })
        .map_err(|error| match error {
            ManaPaymentError::InsufficientMana => EffectError::InsufficientMana,
            ManaPaymentError::PaymentRequired => EffectError::ManaPaymentRequired,
            other => EffectError::InvalidManaPayment(other),
        })?;
        for kind in ManaType::ALL {
            let selected = chosen
                .iter()
                .filter(|((color, _), _)| *color == kind)
                .fold(RulesInteger::from(0), |sum, (_, amount)| sum.plus(amount));
            let total = spend
                .iter()
                .find(|row| row.mana_type == kind)
                .map(|row| row.amount.clone())
                .unwrap_or_else(|| 0.into());
            if selected > total {
                return Err(EffectError::InvalidManaPayment(
                    ManaPaymentError::WrongTotal,
                ));
            }
        }
        Ok(spend)
    }
    fn restriction_allows_ability(
        &self,
        rule: crate::mana::ManaRestriction,
        source: &ObjectCharacteristics,
    ) -> bool {
        match rule {
            crate::mana::ManaRestriction::ArtifactSpellOrAbility
            | crate::mana::ManaRestriction::ArtifactAbility => source.has_type(CardType::Artifact),
            crate::mana::ManaRestriction::NotNonartifactSpell
            | crate::mana::ManaRestriction::ArtifactSpellOrAnyAbility
            | crate::mana::ManaRestriction::ActivatedAbility
            | crate::mana::ManaRestriction::NotSpellFromHand => true,
            crate::mana::ManaRestriction::CreatureSpellOrAbility
            | crate::mana::ManaRestriction::CreatureAbility => source.is_creature(),
            crate::mana::ManaRestriction::EquipmentOrEquipAbility => source
                .current_artifact_subtypes
                .contains(&crate::program::ArtifactSubtype::Equipment),
            crate::mana::ManaRestriction::CreatureSpellOrAbilityOfType(Some(kind)) => {
                source.is_creature() && source.current_creature_subtypes.contains(&kind)
            }
            crate::mana::ManaRestriction::ColorlessEldraziSpellOrAbility => self
                .creature_vocabulary
                .resolve_label("Eldrazi")
                .is_some_and(|kind| {
                    source.current_colors.is_empty()
                        && source.current_creature_subtypes.contains(&kind)
                }),
            _ => false,
        }
    }
    /// Floating restricted mana, grouped by player, color and spending rule.
    pub fn restricted_mana(
        &self,
    ) -> &BTreeMap<(SeatId, ManaType, crate::mana::ManaRestriction), RulesInteger> {
        &self.restricted_mana
    }
    fn debit_restricted_mana(
        &mut self,
        actor: SeatId,
        restricted: &[crate::mana::RestrictedManaSpend],
    ) -> Result<(), EffectError> {
        for item in restricted {
            let key = (actor, item.mana_type, item.restriction);
            let amount = self
                .restricted_mana
                .get(&key)
                .cloned()
                .ok_or(EffectError::InsufficientMana)?;
            if amount < item.amount {
                return Err(EffectError::InsufficientMana);
            }
            let remaining = amount.minus(&item.amount);
            if remaining.is_positive() {
                self.restricted_mana.insert(key, remaining);
            } else {
                self.restricted_mana.remove(&key);
            }
        }
        Ok(())
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
        self.activate_ability_on_turn(
            expected,
            actor,
            ActivationRequest {
                granting_effect: None,
                chosen_x: None,
                source,
                ability,
                origin: crate::activated::ActivationOrigin::Targeted(target),
            },
            payment,
            None,
        )
    }

    pub(crate) fn activate_ability_on_turn(
        &self,
        expected: u64,
        actor: SeatId,
        request: ActivationRequest,
        payment: Option<ManaPayment>,
        turn: Option<&TurnScheduler>,
    ) -> Result<AbilityActivationTransition, EffectError> {
        self.activate_ability_on_turn_restricted(expected, actor, request, payment, &[], turn)
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
            ManaActivationResult::PendingRule { .. } => Err(EffectError::SelectionRequired),
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
        self.begin_mana_with_parent(
            expected,
            actor,
            source,
            ability,
            None,
            ManaActivationChoices::default(),
        )
    }
    /// Activate a mana ability with an explicit mana allocation for its cost.
    ///
    /// # Errors
    /// Rejects illegal source, timing or payment without changing state.
    pub fn begin_mana_activation_with_payment(
        &self,
        expected: u64,
        actor: SeatId,
        source: ObjectId,
        ability: crate::mana::ManaAbilityRef,
        payment: &ManaPayment,
    ) -> Result<ManaActivationProgress, EffectError> {
        self.begin_mana_with_parent(
            expected,
            actor,
            source,
            ability,
            None,
            ManaActivationChoices {
                payment: Some(payment),
                bundle: None,
                cost_objects: None,
                recipient: None,
                restricted: None,
            },
        )
    }
    /// Activate a hybrid filter with an explicit payment and exact output bundle.
    ///
    /// # Errors
    /// Rejects illegal cost, output, source or timing without changing state.
    pub fn begin_hybrid_mana_activation(
        &self,
        expected: u64,
        actor: SeatId,
        source: ObjectId,
        ability: crate::mana::ManaAbilityRef,
        payment: &ManaPayment,
        bundle: &BTreeMap<ManaType, RulesInteger>,
    ) -> Result<ManaActivationProgress, EffectError> {
        self.begin_mana_with_parent(
            expected,
            actor,
            source,
            ability,
            None,
            ManaActivationChoices {
                payment: Some(payment),
                bundle: Some(bundle),
                cost_objects: None,
                recipient: None,
                restricted: None,
            },
        )
    }
    /// Activate a tap mana ability and select its exact multi-color output.
    ///
    /// # Errors
    /// Rejects an illegal bundle, source or timing without changing state.
    pub fn begin_mana_activation_with_bundle(
        &self,
        expected: u64,
        actor: SeatId,
        source: ObjectId,
        ability: crate::mana::ManaAbilityRef,
        bundle: &BTreeMap<ManaType, RulesInteger>,
    ) -> Result<ManaActivationProgress, EffectError> {
        self.begin_mana_with_parent(
            expected,
            actor,
            source,
            ability,
            None,
            ManaActivationChoices {
                payment: None,
                bundle: Some(bundle),
                cost_objects: None,
                recipient: None,
                restricted: None,
            },
        )
    }
    /// Activate a mana ability with explicitly selected non-source cost objects.
    ///
    /// # Errors
    /// Rejects an invalid source, cost selection or activation context without publishing it.
    pub fn begin_mana_activation_with_cost_objects(
        &self,
        expected: u64,
        actor: SeatId,
        source: ObjectId,
        ability: crate::mana::ManaAbilityRef,
        objects: &[ObjectId],
    ) -> Result<ManaActivationProgress, EffectError> {
        self.begin_mana_with_parent(
            expected,
            actor,
            source,
            ability,
            None,
            ManaActivationChoices {
                payment: None,
                bundle: None,
                cost_objects: Some(objects),
                recipient: None,
                restricted: None,
            },
        )
    }
    /// Activate a paid mana ability with an explicit restricted-pool allocation.
    ///
    /// # Errors
    /// Rejects an unavailable allocation, nonqualifying source, illegal bundle or stale context.
    // Public API entry point kept flat to preserve existing caller signatures.
    #[allow(clippy::too_many_arguments)]
    pub fn begin_mana_activation_with_restricted_payment(
        &self,
        expected: u64,
        actor: SeatId,
        source: ObjectId,
        ability: crate::mana::ManaAbilityRef,
        payment: &ManaPayment,
        restricted: &[crate::mana::RestrictedManaSpend],
        bundle: Option<&BTreeMap<ManaType, RulesInteger>>,
    ) -> Result<ManaActivationProgress, EffectError> {
        self.begin_mana_with_parent(
            expected,
            actor,
            source,
            ability,
            None,
            ManaActivationChoices {
                payment: Some(payment),
                bundle,
                cost_objects: None,
                recipient: None,
                restricted: Some(restricted),
            },
        )
    }
    /// Activate a mana ability that chooses another player to receive the produced mana.
    pub fn begin_mana_activation_for_player(
        &self,
        expected: u64,
        actor: SeatId,
        source: ObjectId,
        ability: crate::mana::ManaAbilityRef,
        recipient: SeatId,
    ) -> Result<ManaActivationProgress, EffectError> {
        self.begin_mana_with_parent(
            expected,
            actor,
            source,
            ability,
            None,
            ManaActivationChoices {
                recipient: Some(recipient),
                ..ManaActivationChoices::default()
            },
        )
    }
    fn begin_mana_with_parent(
        &self,
        expected: u64,
        actor: SeatId,
        source: ObjectId,
        ability: crate::mana::ManaAbilityRef,
        authorized_parent: Option<ManaParent>,
        choices: ManaActivationChoices<'_>,
    ) -> Result<ManaActivationProgress, EffectError> {
        let parent = match &authorized_parent {
            Some(parent) => parent.clone(),
            None => self.mana_activation_parent(actor)?,
        };
        let (mut next, result, append) = self
            .begin_mana_with_parent_inner(
                expected,
                actor,
                source,
                ability,
                authorized_parent,
                choices,
            )?
            .into_parts();
        if matches!(result, ManaActivationResult::Complete { .. })
            && let Some(frame) = next.schedule_triggered_mana(parent)
        {
            return Ok(ManaActivationProgress::new(
                next,
                ManaActivationResult::Pending { frame },
                append,
            ));
        }
        Ok(ManaActivationProgress::new(next, result, append))
    }
    fn schedule_triggered_mana(&mut self, parent: ManaParent) -> Option<ManaColorChoice> {
        if self.triggered_mana_queue.is_empty() {
            return None;
        }
        let (source, id, controller, chooser, colors) = self.triggered_mana_queue.remove(0);
        let frame = ManaColorChoice {
            revision: self.revision,
            source,
            ability: crate::mana::ManaAbilityRef::Printed(id),
            chooser,
            amount: 1.into(),
            colors,
        };
        self.pending_work = Some(PendingWork::ManaColor(Box::new(ManaColorWork {
            frame: frame.clone(),
            parent,
            controller_damage: None,
            triggered: true,
            activation_controller: Some(controller),
            restriction: None,
            sacrificed_source: None,
        })));
        Some(frame)
    }
    pub fn pending_mana_color(&self) -> Option<&ManaColorChoice> {
        match self.pending_work.as_ref() {
            Some(PendingWork::ManaColor(work)) => Some(&work.frame),
            _ => None,
        }
    }
    fn ensure_resolution_or_idle(&self) -> Result<(), EffectError> {
        if self.has_pending_cost_or_linked() {
            return Err(EffectError::SelectionRequired);
        }
        match self.pending_work.as_ref() {
            None | Some(PendingWork::Resolution(_)) => Ok(()),
            Some(
                PendingWork::LibraryTop(_)
                | PendingWork::ManaSacrifice(_)
                | PendingWork::LookTop(_)
                | PendingWork::PermanentEntry(_)
                | PendingWork::CastRecovery(_)
                | PendingWork::BattlefieldEntry(_)
                | PendingWork::SearchEntry(_)
                | PendingWork::LandPlay(_)
                | PendingWork::ActivationCost(_)
                | PendingWork::SelectedActivationCost(_)
                | PendingWork::GraveyardExile(_)
                | PendingWork::GraveyardShuffle(_)
                | PendingWork::ImpulseReplacement(_)
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
            Some(PendingWork::BattlefieldEntry(work)) => Some(&work.cursor),
            Some(PendingWork::GraveyardExile(work)) => Some(&work.cursor),
            Some(PendingWork::GraveyardShuffle(work)) => Some(&work.cursor),
            Some(PendingWork::SearchEntry(work)) => Some(&work.cursor),
            Some(PendingWork::ImpulseReplacement(work)) => Some(&work.cursor),
            Some(PendingWork::LibraryTop(work)) => Some(&work.cursor),
            Some(PendingWork::LookTop(work)) => Some(&work.cursor),
            Some(PendingWork::ResolutionSelection(work)) => Some(&work.cursor),
            Some(PendingWork::Resolution(cursor)) => Some(cursor),
            Some(PendingWork::TokenEntry(work)) => Some(&work.cursor),
            _ => None,
        }
    }
    fn resolution_cursor_mut(&mut self) -> Option<&mut ResolutionCursor> {
        match self.pending_work.as_mut() {
            Some(PendingWork::BattlefieldEntry(work)) => Some(&mut work.cursor),
            Some(PendingWork::ImpulseReplacement(work)) => Some(&mut work.cursor),
            Some(PendingWork::LibraryTop(work)) => Some(&mut work.cursor),
            Some(PendingWork::LookTop(work)) => Some(&mut work.cursor),
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
        players: (SeatId, SeatId),
        produced: BTreeMap<ManaType, RulesInteger>,
        restriction: Option<crate::mana::ManaRestriction>,
    ) -> ManaProductionRecord {
        let (controller, recipient) = players;
        for (mana_type, amount) in &produced {
            self.mana.insert(
                (recipient, *mana_type),
                self.mana(recipient, *mana_type).plus(amount),
            );
            if let Some(rule) = restriction {
                let key = (recipient, *mana_type, rule);
                let previous = self
                    .restricted_mana
                    .get(&key)
                    .cloned()
                    .unwrap_or_else(|| 0.into());
                self.restricted_mana.insert(key, previous.plus(amount));
            }
        }
        ManaProductionRecord {
            source,
            ability,
            controller,
            recipient: (recipient != controller).then_some(recipient),
            produced,
            restriction,
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
        if matches!(&self.pending_work, Some(PendingWork::ManaColor(work)) if work.controller_damage.is_some())
        {
            return Err(EffectError::WrongPhase);
        }
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
        let parent = work.parent.clone();
        let triggered = work.triggered;
        let activation_controller = work.activation_controller.unwrap_or(actor);
        let restriction = work.restriction;
        let sacrificed_source = work.sacrificed_source;
        next.pending_work = match work.parent {
            ManaParent::Priority | ManaParent::Casting(_) => None,
            ManaParent::CombatPayment(cursor) => Some(PendingWork::Combat(*cursor)),
            ManaParent::Recovery(frame, _) => Some(PendingWork::CastRecovery(Box::new(frame))),
            ManaParent::Payment(parent) => Some(PendingWork::ResolutionPayment(Box::new(parent))),
        };
        let mut append = RulesJournal::default();
        let production = next.credit_mana_at_cut(
            frame.source,
            frame.ability,
            (activation_controller, actor),
            BTreeMap::from([(color, frame.amount.clone())]),
            restriction,
            triggered,
            sacrificed_source,
            &mut append,
        )?;
        next.schedule_triggered_mana(parent);
        Ok(ManaActivationTransition::new(next, production, append))
    }
    pub(crate) fn resume_mana_color_with_rules(
        &self,
        expected: u64,
        frame: &ManaColorChoice,
        actor: SeatId,
        color: ManaType,
    ) -> Result<ManaColorResolution, EffectError> {
        let Some(PendingWork::ManaColor(work)) = self.pending_work.as_ref() else {
            return Err(EffectError::WrongManaColorFrame);
        };
        if work.controller_damage.is_none() {
            let completed = self.resume_mana_color(expected, frame, actor, color)?;
            if let Some(frame) = completed.state().pending_mana_color().cloned() {
                return Ok(ManaColorResolution::Pending(
                    completed.map_result(|_| ManaActivationResult::Pending { frame }),
                ));
            }
            if completed.state().deferred_mana_cast.is_some()
                || completed.state().awaiting_departed_payer()
            {
                let (next, production, append) = completed.into_parts();
                let progress = next.finish_mana_continuation(
                    DamageJournal::default(),
                    vec![],
                    vec![],
                    DrawJournal::default(),
                    append,
                )?;
                return Ok(ManaColorResolution::Damage {
                    production,
                    progress,
                });
            }
            return Ok(ManaColorResolution::Complete(completed));
        }
        let (next, production, damage_journal, append) =
            self.stage_mana_color_damage(expected, frame, actor, color)?;
        let progress = next.progress_rules(
            damage_journal,
            vec![],
            vec![],
            DrawJournal::default(),
            append,
        )?;
        Ok(ManaColorResolution::Damage {
            production,
            progress,
        })
    }
    fn stage_mana_color_damage(
        &self,
        expected: u64,
        frame: &ManaColorChoice,
        actor: SeatId,
        color: ManaType,
    ) -> Result<(Self, ManaProductionRecord, DamageJournal, RulesJournal), EffectError> {
        let Some(PendingWork::ManaColor(work)) = self.pending_work.as_ref() else {
            return Err(EffectError::WrongManaColorFrame);
        };
        let damage = work
            .controller_damage
            .clone()
            .ok_or(EffectError::WrongManaColorFrame)?;
        let revision = self.next_revision(expected)?;
        if &work.frame != frame || frame.revision != self.revision {
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
        let mut append = RulesJournal::default();
        let production = next.credit_mana_at_cut(
            frame.source,
            frame.ability,
            (actor, actor),
            BTreeMap::from([(color, frame.amount.clone())]),
            None,
            false,
            None,
            &mut append,
        )?;
        let mut damage_journal = DamageJournal::default();
        next.install_mana_damage_batch(
            frame.source,
            frame.ability,
            actor,
            damage,
            work.parent,
            &mut damage_journal,
        )?;
        Ok((next, production, damage_journal, append))
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
                    && (object.is_creature() || object.has_type(CardType::Planeswalker))
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
        self.cast_with_resources(
            expected,
            actor,
            spell,
            target,
            crate::mana::CastingPayment {
                permanent_targets: Vec::new(),
                target_division: Vec::new(),
                hybrid_choices: Vec::new(),
                fixed_targets: Vec::new(),
                chosen_x: None,
                modes: Vec::new(),
                exile_cast_type: None,
                alternative_cost_source: None,
                graveyard_permission: None,
                adventure: false,
                split_half: None,
                disturb: false,
                modal_back: false,
                prototype: false,
                prepare: false,
                splice: Vec::new(),
                use_alternative: false,
                use_granted_flashback: false,
                mana: selected,
                restricted_mana: Vec::new(),
                objects: Vec::new(),
                additional: BTreeMap::new(),
            },
            turn,
        )
    }
    pub(crate) fn cast_with_resources(
        &self,
        expected: u64,
        actor: SeatId,
        spell: ObjectId,
        target: Option<TargetRef>,
        payment: crate::mana::CastingPayment,
        turn: Option<&TurnScheduler>,
    ) -> Result<CastTransition, EffectError> {
        self.cast_in_context(expected, actor, spell, target, payment, turn, None)
    }
    fn check_chosen_name_cast_restriction(
        &self,
        spell: &crate::characteristics::ObjectCharacteristics,
        current: &EvaluatedCharacteristics,
    ) -> Result<(), EffectError> {
        if current.objects.values().any(|permanent| {
            let Some(crate::land::EntryChoice::CardName(index)) =
                self.entry_choices.get(&permanent.object)
            else {
                return false;
            };
            Self::permanent_has_restriction(permanent, |rule| match rule {
                crate::characteristics::StaticRestriction::ProhibitCastingChosenName(names) => {
                    names
                        .get(*index)
                        .is_some_and(|name| self.name_rules.has_name(&spell.current_names, name))
                }
                _ => false,
            })
        }) {
            return Err(EffectError::ChosenNameProhibitsCasting);
        }
        Ok(())
    }

    fn finish_cast_payment(
        mut self,
        prepared: PreparedCastPayment,
        turn: Option<&TurnScheduler>,
        mut append: RulesJournal,
    ) -> Result<CastTransition, EffectError> {
        let PreparedCastPayment {
            library_tops,
            entry,
            actor,
            cost,
            unpaid_mana,
            payment,
            life_payment,
            mana_value: resulting_mana_value,
            sunburst,
            copy_source_needed,
            ..
        } = prepared;
        let spend = self.prepare_cast_mana_payment(
            &unpaid_mana,
            payment.mana.as_ref(),
            actor,
            entry,
            &payment.restricted_mana,
        )?;
        if payment.restricted_mana.iter().any(|allocation| {
            matches!(
                allocation.restriction,
                crate::mana::ManaRestriction::CreatureSpellOfTypeUncounterable(_)
            )
        }) {
            self.window.mark_protected_by_spent_mana(entry)?;
        }
        if sunburst {
            self.window.mark_sunburst_colors(entry, &spend)?;
        }
        if let Some(amount) = life_payment {
            self.pay_life_at_cut(
                actor,
                amount,
                crate::occurrence::EventCause::CastLifePayment { entry },
                turn,
                &mut append,
            )?;
        }
        let spell_extra_costs: Vec<_> = self
            .window
            .entries()
            .iter()
            .find(|e| e.id() == entry)
            .and_then(|e| self.window.spell_object(e))
            .and_then(|id| self.copiable_program(id))
            .map(|p| p.extra_cast_costs.clone())
            .unwrap_or_default();
        if !spell_extra_costs.is_empty() {
            let extra_objects: Vec<ObjectId> = payment
                .objects
                .iter()
                .filter_map(|r| match r {
                    crate::mana::CastResource::ExtraCost(id) => Some(*id),
                    _ => None,
                })
                .collect();
            let chosen_x = self
                .window
                .entries()
                .iter()
                .find(|e| e.id() == entry)
                .and_then(|e| e.chosen_x())
                .cloned();
            self.pay_extra_costs_at_cut(
                actor,
                None,
                entry,
                crate::occurrence::EventCause::CastLifePayment { entry },
                &spell_extra_costs,
                &extra_objects,
                chosen_x.as_ref(),
                turn,
                &mut append,
            )?;
        }
        let remaining = self.pay_cast_resources(&payment.objects, entry, turn, &mut append)?;
        let receipt = ManaPaymentRecord {
            from_lots: payment
                .mana
                .as_ref()
                .map_or_else(Default::default, |allocation| allocation.from_lots.clone()),
            source: entry,
            payer: actor,
            cost,
            spend,
            restricted_mana: payment.restricted_mana,
        };
        if self.has_pending_cost_or_linked() {
            self.deferred_cast = Some(DeferredCast {
                library_tops,
                resources: remaining,
                entry,
                actor,
                receipt: receipt.clone(),
                mana_value: resulting_mana_value,
                copy_source_needed,
            });
            return Ok(CastTransition::new(self, receipt, append));
        }
        self.debit_mana_at_cut(&receipt, true, turn, &mut append)?;
        let copy_source = if copy_source_needed {
            let prior = self
                .cast_history
                .as_ref()
                .map_or_else(|| 0.into(), |history| history.spells.clone());
            Some(Box::new(self.capture_copy_source(entry, prior)?))
        } else {
            None
        };
        let completed = self.rules_snapshot(turn)?;
        self.emit_from(
            crate::occurrence::EventCause::CastCompleted { entry },
            &completed,
            turn,
            &mut append,
            |_, current, _| {
                Ok(vec![crate::occurrence::RulesOccurrence::CastCompleted(
                    Box::new({
                        let mut cast = current.cast_occurrence(
                            entry,
                            actor,
                            &receipt.spend,
                            resulting_mana_value,
                        )?;
                        cast.copy_source = copy_source;
                        cast
                    }),
                )])
            },
        )?;
        Ok(CastTransition::new(self, receipt, append))
    }
    /// Recompute the per-player limit against the current battlefield before each land play.
    pub(crate) fn land_play_allowance(&self, actor: SeatId) -> Result<RulesInteger, EffectError> {
        use crate::characteristics::{CastPermissionRule, StaticContinuousAbility};
        let current = self.evaluated_characteristics()?;
        let cant_play_land = current.objects.values().any(|permanent| {
            if permanent.zone != ObjectZone::Battlefield {
                return false;
            }
            let Some(ctrl) = permanent.controller else {
                return false;
            };
            Self::permanent_has_restriction(permanent, |rule| {
                let crate::characteristics::StaticRestriction::CantPlayLands(rel) = rule else {
                    return false;
                };
                match rel {
                    crate::program::PlayerRelation::Controller => actor == ctrl,
                    crate::program::PlayerRelation::Opponent => self
                        .combat_rules
                        .opponents(self.window.participants(), ctrl)
                        .is_ok_and(|opps| opps.contains(&actor)),
                    crate::program::PlayerRelation::AnyParticipant => true,
                }
            })
        }) || self.temporary_rules.iter().any(|active| {
            if active.consumed {
                return false;
            }
            if matches!(
                active.rule,
                crate::program::TemporaryRuleKind::CantPlayLands { .. }
            ) {
                return active.resolved_players.contains(&actor);
            }
            let Some(ab) = self.active_temporary_static(active, &current) else {
                return false;
            };
            let StaticContinuousAbility::Restriction {
                rule: crate::characteristics::StaticRestriction::CantPlayLands(rel),
                ..
            } = ab
            else {
                return false;
            };
            match rel {
                crate::program::PlayerRelation::Controller => actor == active.controller,
                crate::program::PlayerRelation::Opponent => self
                    .combat_rules
                    .opponents(self.window.participants(), active.controller)
                    .is_ok_and(|opps| opps.contains(&actor)),
                crate::program::PlayerRelation::AnyParticipant => true,
            }
        });
        if cant_play_land {
            return Ok(0.into());
        }
        let extra = current.objects.values().flat_map(|permanent| {
            let is_battlefield = permanent.zone == ObjectZone::Battlefield;
            let is_actor = permanent.controller == Some(actor);
            permanent
                .printed_text
                .continuous
                .iter()
                .chain(&permanent.printed_text.additional_battlefield_abilities)
                .filter_map(move |ability| {
                    if !is_battlefield {
                        return None;
                    }
                    let active = permanent.abilities.continuous == Some(ability.id())
                        || permanent
                            .abilities
                            .additional_battlefield_abilities
                            .contains(&ability.id());
                    if !active {
                        return None;
                    }
                    match ability {
                        StaticContinuousAbility::CastPermission {
                            permission: CastPermissionRule::AdditionalLandPlays(count),
                            ..
                        } if is_actor => Some(count.get()),
                        StaticContinuousAbility::CastPermission {
                            permission: CastPermissionRule::AllPlayersAdditionalLandPlays(count),
                            ..
                        } => Some(count.get()),
                        _ => None,
                    }
                })
        });
        let temp_extra: u32 = self
            .temporary_rules
            .iter()
            .filter_map(|active| match &active.rule {
                crate::program::TemporaryRuleKind::AdditionalLandPlay { count, .. }
                    if active.resolved_players.contains(&actor) =>
                {
                    Some(u32::from(*count))
                }
                _ => None,
            })
            .sum();
        let base = extra.fold(1.into(), |n: RulesInteger, increment| {
            n.plus(&i64::from(increment).into())
        });
        Ok(base.plus(&i64::from(temp_extra).into()))
    }

    /// True if active static or temporary restrictions prevent `player` from losing the game
    /// (or, when `nonpositive_life_only` is true, specifically from losing due to 0 or less life).
    pub(super) fn player_cannot_lose(
        &self,
        evaluated: &EvaluatedCharacteristics,
        player: SeatId,
        nonpositive_life_only: bool,
    ) -> Result<bool, EffectError> {
        use crate::characteristics::{StaticContinuousAbility, StaticRestriction};
        if !self.window.participants().contains(&player) {
            return Ok(false);
        }
        let opponents = self
            .combat_rules
            .opponents(self.window.participants(), player)?;
        let restriction_prevents = |ctrl: SeatId, rule: &StaticRestriction| -> bool {
            match rule {
                StaticRestriction::ControllerCantLoseOpponentsCantWin => ctrl == player,
                StaticRestriction::ControllerCantLoseForZeroLife => {
                    nonpositive_life_only && ctrl == player
                }
                StaticRestriction::ControllerCantWinOpponentsCantLose => opponents.contains(&ctrl),
                _ => false,
            }
        };
        if evaluated.objects.values().any(|perm| {
            perm.zone == ObjectZone::Battlefield
                && perm.controller.is_some_and(|ctrl| {
                    Self::permanent_has_restriction(perm, |rule| restriction_prevents(ctrl, rule))
                })
        }) {
            return Ok(true);
        }
        Ok(self.temporary_rules.iter().any(|active| {
            self.active_temporary_static(active, evaluated)
                .is_some_and(|ab| {
                    matches!(
                        ab,
                        StaticContinuousAbility::Restriction { rule, .. }
                            if restriction_prevents(active.controller, rule)
                    )
                })
        }))
    }

    /// True if active static or temporary restrictions prevent `player` from winning the game.
    pub(super) fn player_cannot_win(
        &self,
        evaluated: &EvaluatedCharacteristics,
        player: SeatId,
    ) -> Result<bool, EffectError> {
        use crate::characteristics::{StaticContinuousAbility, StaticRestriction};
        if !self.window.participants().contains(&player) {
            return Ok(false);
        }
        let opponents = self
            .combat_rules
            .opponents(self.window.participants(), player)?;
        let restriction_prevents = |ctrl: SeatId, rule: &StaticRestriction| -> bool {
            match rule {
                StaticRestriction::ControllerCantLoseOpponentsCantWin => opponents.contains(&ctrl),
                StaticRestriction::ControllerCantWinOpponentsCantLose => ctrl == player,
                _ => false,
            }
        };
        if evaluated.objects.values().any(|perm| {
            perm.zone == ObjectZone::Battlefield
                && perm.controller.is_some_and(|ctrl| {
                    Self::permanent_has_restriction(perm, |rule| restriction_prevents(ctrl, rule))
                })
        }) {
            return Ok(true);
        }
        Ok(self.temporary_rules.iter().any(|active| {
            self.active_temporary_static(active, evaluated)
                .is_some_and(|ab| {
                    matches!(
                        ab,
                        StaticContinuousAbility::Restriction { rule, .. }
                            if restriction_prevents(active.controller, rule)
                    )
                })
        }))
    }

    /// True if active static or temporary `PreventCounters` rules forbid placing `kind` counters on `object`.
    pub(super) fn permanent_counter_prevented(
        &self,
        evaluated: &EvaluatedCharacteristics,
        object: ObjectId,
        kind: CounterKind,
    ) -> bool {
        use crate::characteristics::{StaticContinuousAbility, StaticRestriction};
        for source in evaluated
            .objects
            .values()
            .filter(|perm| perm.zone == ObjectZone::Battlefield)
        {
            let Some(source_ctrl) = source.controller else {
                continue;
            };
            if Self::permanent_has_restriction(source, |rule| {
                matches!(
                    rule,
                    StaticRestriction::PreventCounters {
                        selector: Some(sel),
                        counter_kind,
                        ..
                    } if counter_kind.is_none_or(|k| k == kind)
                        && crate::characteristics::selector_matches_permanent(
                            self.window.cards(),
                            evaluated,
                            source.object,
                            source_ctrl,
                            sel,
                            object,
                            &self.name_rules,
                        )
                )
            }) {
                return true;
            }
        }
        for active in &self.temporary_rules {
            if let Some(StaticContinuousAbility::Restriction {
                rule:
                    StaticRestriction::PreventCounters {
                        selector: Some(sel),
                        counter_kind,
                        ..
                    },
                ..
            }) = self.active_temporary_static(active, evaluated)
                && counter_kind.is_none_or(|k| k == kind)
                && crate::characteristics::selector_matches_permanent(
                    self.window.cards(),
                    evaluated,
                    active.source_object.unwrap_or(ObjectId::SENTINEL),
                    active.controller,
                    sel,
                    object,
                    &self.name_rules,
                )
            {
                return true;
            }
        }
        false
    }

    /// True if active static or temporary `PreventCounters` rules forbid `player` from getting `kind` counters.
    pub(super) fn player_counter_prevented(
        &self,
        evaluated: &EvaluatedCharacteristics,
        player: SeatId,
        kind: CounterKind,
    ) -> Result<bool, EffectError> {
        use crate::characteristics::{StaticContinuousAbility, StaticRestriction};
        let matches_player =
            |rel: crate::program::PlayerRelation, ctrl: SeatId| -> Result<bool, EffectError> {
                Ok(match rel {
                    crate::program::PlayerRelation::Controller => player == ctrl,
                    crate::program::PlayerRelation::Opponent => self
                        .combat_rules
                        .opponents(self.window.participants(), ctrl)?
                        .contains(&player),
                    crate::program::PlayerRelation::AnyParticipant => true,
                })
            };
        for source in evaluated
            .objects
            .values()
            .filter(|perm| perm.zone == ObjectZone::Battlefield)
        {
            let Some(source_ctrl) = source.controller else {
                continue;
            };
            for rule in source
                .active_battlefield_rules()
                .filter_map(|ab| match ab {
                    StaticContinuousAbility::Restriction { rule, .. } => Some(rule),
                    _ => None,
                })
                .chain(&source.granted_restrictions)
            {
                if let StaticRestriction::PreventCounters {
                    players: Some(rel),
                    counter_kind,
                    ..
                } = rule
                    && counter_kind.is_none_or(|k| k == kind)
                    && matches_player(*rel, source_ctrl)?
                {
                    return Ok(true);
                }
            }
        }
        for active in &self.temporary_rules {
            if let Some(StaticContinuousAbility::Restriction {
                rule:
                    StaticRestriction::PreventCounters {
                        players: Some(rel),
                        counter_kind,
                        ..
                    },
                ..
            }) = self.active_temporary_static(active, evaluated)
                && counter_kind.is_none_or(|k| k == kind)
                && matches_player(*rel, active.controller)?
            {
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// Resolve authored continuous-effect / temporary-rule durations against the current
    /// resolving entry, turn number, and source control epoch. Returns `Ok(None)` if a
    /// `WhileControllingAbilitySource` source is no longer controlled on the battlefield.
    pub(super) fn resolve_duration(
        &self,
        entry: &crate::stack_window::StackEntry,
        duration: crate::characteristics::ResolvedDuration,
    ) -> Result<Option<crate::characteristics::ResolvedDuration>, EffectError> {
        use crate::characteristics::ResolvedDuration;
        Ok(match duration {
            ResolvedDuration::UntilControllerNextTurn => {
                Some(ResolvedDuration::UntilPlayerNextTurn {
                    player: entry.controller(),
                    created_in_turn: self.current_turn_number,
                })
            }
            ResolvedDuration::ThroughControllerNextTurn => {
                Some(ResolvedDuration::ThroughNextTurn {
                    player: entry.controller(),
                    created_in_turn: self.current_turn_number,
                })
            }
            ResolvedDuration::WhileControllingAbilitySource => {
                let (StackKind::ActivatedAbility { source }
                | StackKind::TriggeredAbility { source }) = entry.kind()
                else {
                    return Err(EffectError::InconsistentProgram);
                };
                let source = source.id();
                let controller = entry.controller();
                let Some(epoch) = self
                    .window
                    .cards()
                    .get(source)
                    .filter(|card| {
                        card.zone() == ObjectZone::Battlefield
                            && self.window.cards().phased_out_under(source).is_none()
                            && card.controller() == Some(controller)
                    })
                    .and_then(GameObject::control_epoch)
                else {
                    return Ok(None);
                };
                Some(ResolvedDuration::WhileSourceControlled {
                    source,
                    controller,
                    epoch,
                })
            }
            ResolvedDuration::WhileAbilitySourceOnBattlefield => {
                let (StackKind::ActivatedAbility { source }
                | StackKind::TriggeredAbility { source }) = entry.kind()
                else {
                    return Err(EffectError::InconsistentProgram);
                };
                let source = source.id();
                if !self.window.cards().get(source).is_some_and(|card| {
                    card.zone() == ObjectZone::Battlefield
                        && self.window.cards().phased_out_under(source).is_none()
                }) {
                    return Ok(None);
                }
                Some(ResolvedDuration::WhileSourceOnBattlefield { source })
            }
            ResolvedDuration::WhileAbilitySourceTapped => {
                let (StackKind::ActivatedAbility { source }
                | StackKind::TriggeredAbility { source }) = entry.kind()
                else {
                    return Err(EffectError::InconsistentProgram);
                };
                let source = source.id();
                if !self.window.cards().get(source).is_some_and(|card| {
                    card.zone() == ObjectZone::Battlefield
                        && self.window.cards().phased_out_under(source).is_none()
                        && card.tapped() == Some(true)
                }) {
                    return Ok(None);
                }
                Some(ResolvedDuration::WhileSourceTapped { source })
            }
            ResolvedDuration::WhileControllingTappedAbilitySource => {
                let (StackKind::ActivatedAbility { source }
                | StackKind::TriggeredAbility { source }) = entry.kind()
                else {
                    return Err(EffectError::InconsistentProgram);
                };
                let source = source.id();
                let controller = entry.controller();
                let Some(epoch) = self
                    .window
                    .cards()
                    .get(source)
                    .filter(|card| {
                        card.zone() == ObjectZone::Battlefield
                            && self.window.cards().phased_out_under(source).is_none()
                            && card.controller() == Some(controller)
                            && card.tapped() == Some(true)
                    })
                    .and_then(GameObject::control_epoch)
                else {
                    return Ok(None);
                };
                Some(ResolvedDuration::WhileSourceControlledAndTapped {
                    source,
                    controller,
                    epoch,
                })
            }
            other => Some(other),
        })
    }

    /// Ordinary authored land admission; the root owns the per-turn allowance.
    #[cfg(test)]
    pub(crate) fn play_land(
        &self,
        expected: u64,
        actor: SeatId,
        land: ObjectId,
        turn: &TurnScheduler,
    ) -> Result<RulesTransition<Self, LandPlayRecord>, EffectError> {
        self.play_land_with_entry_color(expected, actor, land, turn, None)
    }

    #[cfg(test)]
    pub(crate) fn play_land_with_entry_color(
        &self,
        expected: u64,
        actor: SeatId,
        land: ObjectId,
        turn: &TurnScheduler,
        color: Option<ManaType>,
    ) -> Result<RulesTransition<Self, LandPlayRecord>, EffectError> {
        self.play_land_settled(
            expected,
            actor,
            land,
            turn,
            color.map(crate::land::EntryChoice::Color),
            crate::program::PublicZoneDestination::Battlefield,
        )
    }

    fn play_land_settled(
        &self,
        expected: u64,
        actor: SeatId,
        land: ObjectId,
        turn: &TurnScheduler,
        choice: Option<crate::land::EntryChoice>,
        destination: crate::program::PublicZoneDestination,
    ) -> Result<RulesTransition<Self, LandPlayRecord>, EffectError> {
        let revision = self.next_revision(expected)?;
        self.ensure_resolution_or_idle()?;
        let permitted_outside_hand = self.may_play_from_exile(land, actor)
            || self.library_play_permitted(land, actor)?
            || self.graveyard_land_play_permitted(land, actor)?;
        let flash = self
            .window
            .cards()
            .get(land)
            .filter(|source| source.zone() == ObjectZone::Hand(actor) || permitted_outside_hand)
            .and_then(|source| self.programs.get(&source.card_id()?))
            .and_then(|program| {
                if program.has_type(CardType::Land) {
                    Some(program)
                } else {
                    program
                        .modal_back
                        .as_deref()
                        .filter(|back| back.has_type(CardType::Land))
                }
            })
            .is_some_and(|program| program.printed_keywords.contains(&KeywordAbility::Flash));
        crate::actions::validate_land_timing(&self.window, Some(turn), actor, flash)?;
        let source = self
            .window
            .cards()
            .get(land)
            .ok_or(EffectError::UnknownObject)?;
        if source.zone() != ObjectZone::Hand(actor) && !permitted_outside_hand {
            return Err(EffectError::NotOwnHand);
        }
        let front_program = source
            .card_id()
            .and_then(|card| self.programs.get(&card))
            .ok_or(EffectError::NotLand)?;
        let use_modal_back_land = if front_program.has_type(CardType::Land) {
            false
        } else if front_program
            .modal_back
            .as_deref()
            .is_some_and(|back| back.has_type(CardType::Land))
        {
            true
        } else {
            return Err(EffectError::NotLand);
        };
        let physical = destination
            .physical(Some(actor))
            .ok_or(EffectError::InconsistentProgram)?;
        let (window, mut record) = self.window.record_land_play_replaced(
            self.window.revision(),
            actor,
            land,
            physical,
            permitted_outside_hand,
        )?;
        let before = self.rules_snapshot(Some(turn))?;
        let mut next = Self {
            revision,
            window,
            ..self.clone()
        };
        if destination == crate::program::PublicZoneDestination::Battlefield {
            if use_modal_back_land {
                next.window.enter_transformed(record.after.id())?;
            }
            next.stage_resolution_entry_choice(land, record.after.id())?;
            if let Some(choice @ crate::land::EntryChoice::Chapter(_)) = choice {
                next.entry_choices.insert(record.after.id(), choice);
            }
            if let Some(crate::land::EntryChoice::Color(color)) = choice {
                if !matches!(
                    next.copiable_program(record.after.id())
                        .and_then(Program::permanent)
                        .and_then(|permanent| permanent.entry_tap),
                    Some(
                        crate::land::EntryTap::AlwaysChooseColorExcept(_)
                            | crate::land::EntryTap::ChooseColor
                            | crate::land::EntryTap::ChooseColorExcept(_)
                    )
                ) {
                    return Err(EffectError::IllegalEntryColor);
                }
                next.entry_color_choices.insert(record.after.id(), color);
                if let Some(c) = color.color() {
                    next.characteristics
                        .entry_color_choices
                        .insert(record.after.id(), c);
                }
            }
            if let Some(choice @ crate::land::EntryChoice::Reveal(_)) = choice {
                if !matches!(
                    next.copiable_program(record.after.id())
                        .and_then(Program::permanent)
                        .and_then(|permanent| permanent.entry_tap),
                    Some(crate::land::EntryTap::UnlessRevealHandLandType(_, _))
                ) {
                    return Err(EffectError::InconsistentProgram);
                }
                next.entry_choices.insert(record.after.id(), choice);
            }
            if let Some(crate::land::EntryChoice::CreatureType(kind)) = choice {
                if !matches!(
                    next.copiable_program(record.after.id())
                        .and_then(Program::permanent)
                        .and_then(|permanent| permanent.entry_tap),
                    Some(crate::land::EntryTap::ChooseCreatureType)
                ) || next.creature_vocabulary.label(kind).is_none()
                {
                    return Err(EffectError::IllegalTarget);
                }
                next.entry_creature_type_choices
                    .insert(record.after.id(), kind);
            }
            if let Some(choice @ crate::land::EntryChoice::PayLife(_)) = choice {
                if !matches!(
                    next.copiable_program(record.after.id())
                        .and_then(Program::permanent)
                        .and_then(|permanent| permanent.entry_tap),
                    Some(crate::land::EntryTap::UnlessPayLife(_))
                ) {
                    return Err(EffectError::InconsistentProgram);
                }
                next.entry_choices.insert(record.after.id(), choice);
            }
            if let Some(choice @ crate::land::EntryChoice::OptionIndex(idx)) = choice {
                if !next
                    .copiable_program(record.after.id())
                    .and_then(Program::permanent)
                    .and_then(|permanent| permanent.entry_tap)
                    .is_some_and(|rule| rule.option_labels().is_some())
                {
                    return Err(EffectError::InconsistentProgram);
                }
                next.entry_choices.insert(record.after.id(), choice);
                next.characteristics
                    .entry_option_choices
                    .insert(record.after.id(), idx);
            }
            if let Some(choice @ crate::land::EntryChoice::Opponent(seat)) = choice {
                if !matches!(
                    next.copiable_program(record.after.id())
                        .and_then(Program::permanent)
                        .and_then(|permanent| permanent.entry_tap),
                    Some(
                        crate::land::EntryTap::ChooseOpponent
                            | crate::land::EntryTap::ChoosePlayer
                            | crate::land::EntryTap::SiegeProtector
                            | crate::land::EntryTap::ChosenOpponentControls
                    )
                ) {
                    return Err(EffectError::InconsistentProgram);
                }
                next.entry_choices.insert(record.after.id(), choice);
                next.characteristics
                    .entry_player_choices
                    .insert(record.after.id(), seat);
            }
            next.characteristics
                .register_battlefield_entry(record.after.id())?;
            next.apply_entry_counters(
                &before,
                record.after.id(),
                PowerToughnessCounters::default(),
                0,
            )?;
            record.after = next
                .window
                .cards()
                .get(record.after.id())
                .ok_or(EffectError::InconsistentProgram)?
                .clone();
        }
        let movement = crate::object_identity::ObjectMovement {
            before: record.before.clone(),
            after: record.after.clone(),
        };
        let mut append = RulesJournal::default();
        if record.before.id() != record.after.id() {
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
        }
        record.after = next
            .window
            .cards()
            .get(record.after.id())
            .ok_or(EffectError::InconsistentProgram)?
            .clone();
        Ok(RulesTransition::new(next, record, append))
    }
    fn land_play_entry_frame(
        &self,
        work: &LandPlayWork,
    ) -> Result<Option<EntryChoiceFrame>, EffectError> {
        let chooser = self
            .prospective_entry_controllers
            .get(&work.land)
            .copied()
            .unwrap_or(work.actor);
        let Some(front) = self.prospective_definition(work.land) else {
            return Ok(None);
        };
        let land_face = if front.has_type(CardType::Land) {
            front
        } else if let Some(back) = front
            .modal_back
            .as_deref()
            .filter(|back| back.has_type(CardType::Land))
        {
            back
        } else {
            front
        };
        if land_face
            .permanent()
            .and_then(|permanent| permanent.entry_tap)
            == Some(crate::land::EntryTap::ChooseModalFace)
        {
            return Ok(Some(EntryChoiceFrame {
                revision: self.revision,
                chooser,
                object: work.land,
                rule: crate::land::EntryTap::ChooseModalFace,
                eligible_reveals: Vec::new(),
                card_names: None,
            }));
        }
        self.entry_choice_for_definition(chooser, work.land, land_face, true)
    }
    fn object_entry_choice_frame(
        &self,
        chooser: SeatId,
        object: ObjectId,
    ) -> Result<Option<EntryChoiceFrame>, EffectError> {
        let chooser = self
            .prospective_entry_controllers
            .get(&object)
            .copied()
            .unwrap_or(chooser);
        let Some(definition) = self.prospective_definition(object) else {
            return Ok(None);
        };
        self.entry_choice_for_definition(chooser, object, definition, true)
    }

    fn entry_choice_for_definition(
        &self,
        chooser: SeatId,
        object: ObjectId,
        definition: &Program,
        exclude_entering_card: bool,
    ) -> Result<Option<EntryChoiceFrame>, EffectError> {
        use crate::land::EntryTap;
        let Some(rule) = definition
            .permanent()
            .and_then(|permanent| permanent.entry_tap)
        else {
            return Ok(None);
        };
        let eligible_reveals = match rule {
            EntryTap::UnlessRevealHandLandType(a, b) => {
                let evaluated = self.evaluated_characteristics()?;
                self.window
                    .cards()
                    .iter()
                    .filter(|card| {
                        (!exclude_entering_card || card.id() != object)
                            && card.zone() == ObjectZone::Hand(chooser)
                            && card.card_id().is_some()
                            && evaluated.get(card.id()).is_some_and(|current| {
                                current.has_type(CardType::Land)
                                    && (current.current_land_subtypes.contains(&a.land_type())
                                        || current.current_land_subtypes.contains(&b.land_type()))
                            })
                    })
                    .map(GameObject::id)
                    .collect()
            }
            EntryTap::ReadAhead { .. }
            | EntryTap::AlwaysChooseColorExcept(_)
            | EntryTap::ChooseColor
            | EntryTap::ChooseColorExcept(_)
            | EntryTap::ChooseCreatureType
            | EntryTap::ChooseNonlandCardName
            | EntryTap::ChooseOpponent
            | EntryTap::ChoosePlayer
            | EntryTap::ChooseTwoPlayers
            | EntryTap::ChooseCardType
            | EntryTap::ChooseBasicLandType
            | EntryTap::ChooseOddOrEven
            | EntryTap::ChooseKhansOrDragons
            | EntryTap::ChooseOptions(_)
            | EntryTap::Unleash
            | EntryTap::Riot
            | EntryTap::Tribute(_)
            | EntryTap::SiegeProtector
            | EntryTap::UnlessPayLife(_) => Vec::new(),
            _ => return Ok(None),
        };
        let chooser = if matches!(rule, EntryTap::Tribute(_)) {
            self.opponents_of(chooser)
                .ok()
                .and_then(|ops| ops.first().copied())
                .unwrap_or(chooser)
        } else {
            chooser
        };
        Ok(Some(EntryChoiceFrame {
            revision: self.revision,
            chooser,
            object,
            rule,
            eligible_reveals,
            card_names: definition.nonland_card_names().cloned(),
        }))
    }
    fn valid_entry_choice(
        &self,
        frame: &EntryChoiceFrame,
        choice: crate::land::EntryChoice,
    ) -> bool {
        match (frame.rule, choice) {
            (
                crate::land::EntryTap::AlwaysChooseColorExcept(excluded)
                | crate::land::EntryTap::ChooseColorExcept(excluded),
                crate::land::EntryChoice::Color(color),
            ) => color != excluded && color != ManaType::Colourless,
            (crate::land::EntryTap::ChooseColor, crate::land::EntryChoice::Color(color)) => {
                color != ManaType::Colourless
            }
            (
                crate::land::EntryTap::ReadAhead { final_chapter },
                crate::land::EntryChoice::Chapter(n),
            ) => (1..=final_chapter).contains(&n),
            (
                crate::land::EntryTap::ChooseCreatureType,
                crate::land::EntryChoice::CreatureType(kind),
            ) => self.creature_vocabulary.label(kind).is_some(),
            (
                crate::land::EntryTap::ChooseNonlandCardName,
                crate::land::EntryChoice::CardName(index),
            ) => frame.card_names().get(index.0).is_some(),
            (
                crate::land::EntryTap::SiegeProtector | crate::land::EntryTap::ChooseOpponent,
                crate::land::EntryChoice::Opponent(player),
            ) => self
                .opponents_of(frame.chooser)
                .is_ok_and(|players| players.contains(&player)),
            (crate::land::EntryTap::ChoosePlayer, crate::land::EntryChoice::Opponent(player)) => {
                self.window.participants().contains(&player)
            }
            (rule, crate::land::EntryChoice::OptionIndex(idx))
                if rule.option_labels().is_some() =>
            {
                rule.option_labels()
                    .is_some_and(|labels| usize::from(idx) < labels.len())
            }
            (
                crate::land::EntryTap::ChooseTwoPlayers,
                crate::land::EntryChoice::Players(players),
            ) => self.valid_distinct_players(players),
            (
                crate::land::EntryTap::UnlessRevealHandLandType(_, _),
                crate::land::EntryChoice::Reveal(None),
            ) => true,
            (
                crate::land::EntryTap::UnlessRevealHandLandType(_, _),
                crate::land::EntryChoice::Reveal(Some(object)),
            ) => frame.eligible_reveals.contains(&object),
            (crate::land::EntryTap::UnlessPayLife(_), crate::land::EntryChoice::PayLife(false)) => {
                true
            }
            (
                crate::land::EntryTap::UnlessPayLife(amount),
                crate::land::EntryChoice::PayLife(true),
            ) => {
                amount > 0
                    && self
                        .life
                        .get(frame.chooser)
                        .is_some_and(|life| *life >= i64::from(amount).into())
            }
            _ => false,
        }
    }
    pub(crate) fn begin_land_play(
        &self,
        expected: u64,
        actor: SeatId,
        land: ObjectId,
        turn: &TurnScheduler,
        choice: Option<crate::land::EntryChoice>,
    ) -> Result<RuleProgress, EffectError> {
        let revision = self.next_revision(expected)?;
        self.ensure_resolution_or_idle()?;
        let source = self
            .window
            .cards()
            .get(land)
            .ok_or(EffectError::UnknownObject)?;
        if source.zone() != ObjectZone::Hand(actor)
            && !self.may_play_from_exile(land, actor)
            && !self.library_play_permitted(land, actor)?
            && !self.graveyard_land_play_permitted(land, actor)?
        {
            return Err(EffectError::NotOwnHand);
        }
        let program = source
            .card_id()
            .and_then(|id| self.programs.get(&id))
            .ok_or(EffectError::NotLand)?;
        let land_face = if program.has_type(CardType::Land) {
            program
        } else if let Some(back) = program
            .modal_back
            .as_deref()
            .filter(|back| back.has_type(CardType::Land))
        {
            back
        } else {
            return Err(EffectError::NotLand);
        };
        let flash = land_face.printed_keywords.contains(&KeywordAbility::Flash);
        crate::actions::validate_land_timing(&self.window, Some(turn), actor, flash)?;
        let batch = self.begin_zone_replacements_with_entry(
            zone_replacement::AuthorizedZoneBatch {
                cause: crate::occurrence::EventCause::LandPlay { actor },
                action: crate::program::ZoneActionKind::EffectEnterBattlefield,
                intents: vec![zone_replacement::ZoneMoveIntent {
                    before: land,
                    reason: crate::occurrence::ZoneReason::LandPlay,
                }],
            },
            Some(&BTreeMap::from([(land, actor)])),
        )?;
        let mut next = self.clone();
        next.revision = revision;
        let work = LandPlayWork {
            actor,
            land,
            turn: turn.clone(),
            choice,
            batch,
        };
        if let Some(choice) = work.choice {
            let frame = next
                .land_play_entry_frame(&work)?
                .ok_or(EffectError::IllegalTarget)?;
            if !self.valid_entry_choice(&frame, choice) {
                return Err(EffectError::IllegalTarget);
            }
        }
        next.pending_work = Some(PendingWork::LandPlay(Box::new(work)));
        next.progress_land_play()
    }
    fn progress_land_play(mut self) -> Result<RuleProgress, EffectError> {
        let Some(PendingWork::LandPlay(mut work)) = self.pending_work.take() else {
            return Err(EffectError::WrongResolutionFrame);
        };
        if let Some(frame) = work.batch.progress(self.revision)? {
            self.pending_work = Some(PendingWork::LandPlay(work));
            return Ok(RuleProgress::new(
                self,
                RuleResult::Pending {
                    checks: CheckJournal::default(),
                    frame,
                    journal: DamageJournal::default(),
                    declarations: vec![],
                    assignments: vec![],
                    draws: DrawJournal::default(),
                },
                RulesJournal::default(),
            ));
        }
        let fresh = self.begin_zone_replacements_with_entry(
            zone_replacement::AuthorizedZoneBatch {
                cause: crate::occurrence::EventCause::LandPlay { actor: work.actor },
                action: crate::program::ZoneActionKind::EffectEnterBattlefield,
                intents: vec![zone_replacement::ZoneMoveIntent {
                    before: work.land,
                    reason: crate::occurrence::ZoneReason::LandPlay,
                }],
            },
            Some(&BTreeMap::from([(work.land, work.actor)])),
        )?;
        work.batch.verify_initial(fresh)?;
        self.prospective_entry_copies
            .extend(work.batch.entry_copies());
        self.prospective_entry_controllers
            .extend(work.batch.entry_controllers());
        let destination = work.batch.destination(work.land)?;
        if destination == crate::program::PublicZoneDestination::Battlefield
            && work.choice.is_none()
            && let Some(frame) = self.land_play_entry_frame(&work)?
        {
            self.pending_work = Some(PendingWork::LandPlay(work));
            return Ok(RuleProgress::new(
                self,
                RuleResult::Pending {
                    checks: CheckJournal::default(),
                    frame: EffectDecision::EntryChoice(frame),
                    journal: DamageJournal::default(),
                    declarations: vec![],
                    assignments: vec![],
                    draws: DrawJournal::default(),
                },
                RulesJournal::default(),
            ));
        }
        let mut sacrifices = RulesJournal::default();
        work.batch.commit_devour(&mut self, &mut sacrifices)?;
        let (mut state, record, append) = self
            .play_land_settled(
                self.revision,
                work.actor,
                work.land,
                &work.turn,
                work.choice,
                destination,
            )?
            .into_parts();
        sacrifices.append(append);
        let append = sacrifices;
        state.completed_land_play = Some(record);
        Ok(RuleProgress::new(
            state,
            RuleResult::Complete {
                checks: CheckJournal::default(),
                completion: RuleCompletion::LandPlay,
                journal: DamageJournal::default(),
                declarations: vec![],
                assignments: vec![],
                permanent_entry: None,
                draws: DrawJournal::default(),
            },
            append,
        ))
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
                .is_some_and(|body| {
                    body.target == BodyTarget::None
                        || body
                            .permanent_target_bounds()
                            .or_else(|| body.ordered_any_target_bounds())
                            .is_some_and(|(minimum, _)| minimum == 0)
                })
                && entry.targets().targets().is_empty()
            {
                let mut next = self.clone();
                next.revision = revision;
                let mut results = ResolutionResults::default();
                let mut exiled_target = None;
                if let Some(StackPayload::Instructions(payload)) = next.payloads.get(&entry.id())
                    && let Some(ctx) = &payload.reflexive_context
                {
                    results.last_sacrificed_creature = ctx.last_sacrificed_creature.clone();
                    results.discarded_types = ctx.discarded_types.clone();
                    results.last_moved_objects = ctx.last_moved_objects.clone();
                    results.last_moved_controllers = ctx.last_moved_controllers.clone();
                    results.last_action_count = ctx.last_action_count.clone();
                    exiled_target = ctx.exiled_target.clone();
                }
                next.pending_work = Some(PendingWork::Resolution(ResolutionCursor {
                    revealed_piles: None,
                    chosen_opponent: None,
                    chosen_damage_recipient: None,
                    damage_offer_opponents: None,
                    vowed_creatures: Default::default(),
                    meld_pair: None,
                    manifest_remaining: None,
                    random_selection: None,
                    results,
                    chosen_effect_color: None,
                    chosen_option_index: None,
                    chosen_distinct_colors: Vec::new(),
                    chosen_mana_split: None,
                    tempting_token_offer: None,
                    destroyed_permanents: None,
                    chosen_loyalty_removals: None,
                    discarded_nonland_mana_value: None,
                    impulse: ImpulseState::default(),
                    entry: entry.id(),
                    instruction: 0,
                    last_created_tokens: Vec::new(),
                    chosen_token_equipment: None,
                    found_spells: Vec::new(),
                    largest_discard: None,
                    exiled_target,
                    remembered_permanent: None,
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
            if self.window.spell_object(entry).is_none() {
                return Err(EffectError::InconsistentProgram);
            }
            let mut next = self.clone();
            next.revision = revision;
            return next.resolve_permanent_entry(entry);
        }
        let entry = self
            .window
            .checking_targets()
            .ok_or(EffectError::WrongPhase)?
            .clone();
        if entry.targets().targets().is_empty() {
            return Err(EffectError::InconsistentProgram);
        }
        let current = self.evaluated_characteristics()?;
        let source = self.spell_or_ability_source(&entry, &current);
        let mut next = self.clone();
        next.revision = revision;
        let mut checks = Vec::with_capacity(entry.targets().targets().len());
        for bound in entry.targets().targets() {
            let choice = bound.choice();
            let legal = self.existing_entry_target_legal(&entry, choice, &current, &source)?;
            checks.push(TargetCheck {
                target: choice.id,
                legal,
            });
        }
        next.window =
            next.window
                .check_resolution_targets(next.window.revision(), entry.id(), checks)?;
        if next
            .window
            .target_assessment()
            .ok_or(EffectError::InconsistentProgram)?
            .permission()
            == ResolutionPermission::AllTargetsIllegal
        {
            if next.mutating_spell(&entry).is_some()
                || (matches!(entry.kind(), StackKind::CardSpell(object) if next.characteristics.bestowed_spells.remove(object)))
            {
                next.window.resolve_as_untargeted_permanent(entry.id())?;
                return next.resolve_permanent_entry(&entry);
            }
            let mut append = RulesJournal::default();
            next.complete_at_cut(
                &entry,
                crate::occurrence::EntryCompletion::AllTargetsIllegal,
                next.entry_exit(&entry, false),
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
        if let Some(frame) = next.mutation_frame() {
            return Ok(RuleProgress::new(
                next,
                RuleResult::Pending {
                    frame: EffectDecision::Mutation(frame),
                    checks: CheckJournal::default(),
                    journal: DamageJournal::default(),
                    declarations: vec![],
                    assignments: vec![],
                    draws: DrawJournal::default(),
                },
                RulesJournal::default(),
            ));
        }
        if self.aura_on_stack(&entry).is_some() {
            return next.resolve_permanent_entry(&entry);
        }
        let mut results = ResolutionResults::default();
        let mut exiled_target = None;
        if let Some(StackPayload::Instructions(payload)) = next.payloads.get(&entry.id())
            && let Some(ctx) = &payload.reflexive_context
        {
            results.last_sacrificed_creature = ctx.last_sacrificed_creature.clone();
            results.discarded_types = ctx.discarded_types.clone();
            results.last_moved_objects = ctx.last_moved_objects.clone();
            results.last_moved_controllers = ctx.last_moved_controllers.clone();
            results.last_action_count = ctx.last_action_count.clone();
            exiled_target = ctx.exiled_target.clone();
        }
        for bound in entry.targets().targets() {
            if let TargetRef::Object(id) = bound.choice().target
                && let Some(lki) = next.capture_target_lki(id, &current)
            {
                results.target_lki.insert(id, lki);
            }
        }
        next.pending_work = Some(PendingWork::Resolution(ResolutionCursor {
            revealed_piles: None,
            chosen_opponent: None,
            chosen_damage_recipient: None,
            damage_offer_opponents: None,
            vowed_creatures: Default::default(),
            meld_pair: None,
            manifest_remaining: None,
            random_selection: None,
            results,
            chosen_effect_color: None,
            chosen_option_index: None,
            chosen_distinct_colors: Vec::new(),
            chosen_mana_split: None,
            tempting_token_offer: None,
            destroyed_permanents: None,
            chosen_loyalty_removals: None,
            discarded_nonland_mana_value: None,
            impulse: ImpulseState::default(),
            entry: entry.id(),
            instruction: 0,
            last_created_tokens: Vec::new(),
            chosen_token_equipment: None,
            found_spells: Vec::new(),
            largest_discard: None,
            exiled_target,
            remembered_permanent: None,
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
    fn resolve_permanent_entry(
        mut self,
        entry: &crate::stack_window::StackEntry,
    ) -> Result<RuleProgress, EffectError> {
        let object = self
            .window
            .spell_object(entry)
            .ok_or(EffectError::InconsistentProgram)?;
        if !self.settled_entry_destinations.contains_key(&object) {
            let batch = self.begin_zone_replacements_with_entry(
                zone_replacement::AuthorizedZoneBatch {
                    cause: crate::occurrence::EventCause::ResolutionCompleted {
                        entry: entry.id(),
                        completion: crate::occurrence::EntryCompletion::Resolved,
                    },
                    action: crate::program::ZoneActionKind::EffectEnterBattlefield,
                    intents: vec![zone_replacement::ZoneMoveIntent {
                        before: object,
                        reason: crate::occurrence::ZoneReason::PermanentResolution,
                    }],
                },
                Some(&BTreeMap::from([(object, entry.controller())])),
            )?;
            self.pending_work = Some(PendingWork::PermanentEntry(Box::new(PermanentEntryWork {
                entry: entry.clone(),
                object,
                batch,
            })));
            return self.progress_permanent_entry();
        }
        let mut destination = self
            .settled_entry_destinations
            .remove(&object)
            .ok_or(EffectError::InconsistentProgram)?;
        if destination == crate::program::PublicZoneDestination::Battlefield
            && let Some(frame) = self.copied_spell_aura_frame(entry, object)?
        {
            if frame.domain.is_empty() {
                self.prospective_entry_copies.remove(&object);
                self.prospective_entry_controllers.remove(&object);
                destination = crate::program::PublicZoneDestination::Graveyard;
            } else {
                self.settled_entry_destinations.insert(object, destination);
                return Ok(RuleProgress::new(
                    self,
                    RuleResult::Pending {
                        checks: CheckJournal::default(),
                        frame: EffectDecision::LinkedAuraChoice(frame),
                        declarations: vec![],
                        assignments: vec![],
                        journal: DamageJournal::default(),
                        draws: DrawJournal::default(),
                    },
                    RulesJournal::default(),
                ));
            }
        }
        if destination == crate::program::PublicZoneDestination::Battlefield
            && let Some(frame) = self.entry_choice_frame()?
        {
            self.settled_entry_destinations.insert(object, destination);
            return Ok(RuleProgress::new(
                self,
                RuleResult::Pending {
                    checks: CheckJournal::default(),
                    frame: EffectDecision::EntryChoice(frame),
                    declarations: vec![],
                    assignments: vec![],
                    journal: DamageJournal::default(),
                    draws: DrawJournal::default(),
                },
                RulesJournal::default(),
            ));
        }
        let mut append = RulesJournal::default();
        let movement = self
            .complete_at_cut(
                entry,
                crate::occurrence::EntryCompletion::Resolved,
                Some(
                    destination
                        .physical(Some(entry.controller()))
                        .ok_or(EffectError::InconsistentProgram)?,
                ),
                &mut append,
            )?
            .ok_or(EffectError::InconsistentProgram)?;
        Ok(RuleProgress::new(
            self,
            RuleResult::Complete {
                checks: CheckJournal::default(),
                completion: RuleCompletion::Resolution,
                declarations: vec![],
                assignments: vec![],
                permanent_entry: (destination
                    == crate::program::PublicZoneDestination::Battlefield)
                    .then_some(PermanentEntryRecord {
                        entry: entry.id(),
                        before: movement.before,
                        after: movement.after,
                    }),
                journal: DamageJournal::default(),
                draws: DrawJournal::default(),
            },
            append,
        ))
    }
    fn progress_permanent_entry(mut self) -> Result<RuleProgress, EffectError> {
        let Some(PendingWork::PermanentEntry(mut work)) = self.pending_work.take() else {
            return Err(EffectError::WrongResolutionFrame);
        };
        if let Some(frame) = work.batch.progress(self.revision)? {
            self.pending_work = Some(PendingWork::PermanentEntry(work));
            return Ok(RuleProgress::new(
                self,
                RuleResult::Pending {
                    checks: CheckJournal::default(),
                    frame,
                    journal: DamageJournal::default(),
                    declarations: vec![],
                    assignments: vec![],
                    draws: DrawJournal::default(),
                },
                RulesJournal::default(),
            ));
        }
        let fresh = self.begin_zone_replacements_with_entry(
            zone_replacement::AuthorizedZoneBatch {
                cause: crate::occurrence::EventCause::ResolutionCompleted {
                    entry: work.entry.id(),
                    completion: crate::occurrence::EntryCompletion::Resolved,
                },
                action: crate::program::ZoneActionKind::EffectEnterBattlefield,
                intents: vec![zone_replacement::ZoneMoveIntent {
                    before: work.object,
                    reason: crate::occurrence::ZoneReason::PermanentResolution,
                }],
            },
            Some(&BTreeMap::from([(work.object, work.entry.controller())])),
        )?;
        work.batch.verify_initial(fresh)?;
        self.prospective_entry_copies
            .extend(work.batch.entry_copies());
        self.prospective_entry_controllers
            .extend(work.batch.entry_controllers());
        self.settled_entry_destinations
            .insert(work.object, work.batch.destination(work.object)?);
        let mut append = RulesJournal::default();
        work.batch.commit_devour(&mut self, &mut append)?;
        let (state, result, entry_append) = self.resolve_permanent_entry(&work.entry)?.into_parts();
        append.append(entry_append);
        Ok(RuleProgress::new(state, result, append))
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
    fn keyword_entry_candidate(
        &self,
        instruction: &Instruction,
        entry: &crate::stack_window::StackEntry,
    ) -> Result<Option<(ObjectId, SeatId, crate::occurrence::ZoneReason)>, EffectError> {
        use crate::occurrence::ZoneReason;
        match instruction {
            Instruction::Unearth
            | Instruction::Ninjutsu
            | Instruction::ExileUpToTwoGraveyardTargetsGainLife
            | Instruction::ReturnUpToOneOwnGraveyardCreatureToHand
            | Instruction::ReturnSourceGraveyardToBattlefield { .. } => {
                let (StackKind::ActivatedAbility { source }
                | StackKind::TriggeredAbility { source }) = entry.kind()
                else {
                    return Err(EffectError::InconsistentProgram);
                };
                let (zone, reason) = if matches!(
                    instruction,
                    Instruction::ReturnSourceGraveyardToBattlefield { .. }
                ) {
                    (
                        ObjectZone::Graveyard(entry.controller()),
                        ZoneReason::ReturnFromGraveyardToBattlefield,
                    )
                } else if *instruction == Instruction::Unearth {
                    (
                        ObjectZone::Graveyard(entry.controller()),
                        ZoneReason::UnearthReturn,
                    )
                } else {
                    (source.zone(), ZoneReason::NinjutsuEntry)
                };
                Ok(self
                    .window
                    .cards()
                    .get(source.id())
                    .filter(|object| object.zone() == zone)
                    .map(|_| (source.id(), entry.controller(), reason)))
            }
            Instruction::ResolveCounterKeyword(
                keyword @ (crate::program::CounterKeyword::Persist
                | crate::program::CounterKeyword::Undying),
            ) => {
                let Some(binding) = self
                    .execution_bindings(entry.id())
                    .and_then(|b| b.trigger.as_ref())
                else {
                    return Ok(None);
                };
                let [cause] = binding.causes.members() else {
                    return Ok(None);
                };
                let crate::occurrence::TriggerRelated::ZoneChange { before, after } =
                    &cause.related
                else {
                    return Ok(None);
                };
                if !keyword.eligible_at_death(before) {
                    return Ok(None);
                }
                let object = after.object.id();
                let Some(card) = self.window.cards().get(object) else {
                    return Ok(None);
                };
                let owner = card.owner();
                if card.zone() != ObjectZone::Graveyard(owner) || card.card_id().is_none() {
                    return Ok(None);
                }
                let reason = if *keyword == crate::program::CounterKeyword::Persist {
                    ZoneReason::PersistReturn
                } else {
                    ZoneReason::UndyingReturn
                };
                Ok(Some((object, owner, reason)))
            }
            _ => Ok(None),
        }
    }
    fn entry_choice_frame(&self) -> Result<Option<EntryChoiceFrame>, EffectError> {
        let entry = self.window.resolving().ok_or(EffectError::WrongPhase)?;
        let (object, chooser) = if self.resolution_cursor().is_some() {
            match self.current_instruction()? {
                Instruction::ReturnOwnGraveyardCreatureToBattlefield
                | Instruction::ReturnOwnGraveyardPermanentToBattlefield { .. }
                | Instruction::ReturnAnyGraveyardCreatureBlackSubtype(_)
                | Instruction::ReturnAnyGraveyardCreatureOrPlaneswalker => {
                    let TargetRef::Object(object) = self.instruction_target(entry)?.choice().target
                    else {
                        return Ok(None);
                    };
                    (object, entry.controller())
                }
                operation @ (Instruction::Unearth
                | Instruction::Ninjutsu
                | Instruction::ExileUpToTwoGraveyardTargetsGainLife
                | Instruction::ReturnUpToOneOwnGraveyardCreatureToHand
                | Instruction::ReturnSourceGraveyardToBattlefield { .. }) => {
                    let (StackKind::ActivatedAbility { source }
                    | StackKind::TriggeredAbility { source }) = entry.kind()
                    else {
                        return Ok(None);
                    };
                    let zone = if matches!(
                        operation,
                        Instruction::Unearth
                            | Instruction::ReturnUpToOneRecentGraveyardCard
                            | Instruction::ExileUpToTwoGraveyardTargetsGainLife
                            | Instruction::ReturnUpToOneOwnGraveyardCreatureToHand
                            | Instruction::ReturnSourceGraveyardToBattlefield { .. }
                    ) {
                        ObjectZone::Graveyard(entry.controller())
                    } else {
                        ObjectZone::Hand(entry.controller())
                    };
                    if self
                        .window
                        .cards()
                        .get(source.id())
                        .is_none_or(|card| card.zone() != zone)
                    {
                        return Ok(None);
                    }
                    (source.id(), entry.controller())
                }
                Instruction::ResolveCounterKeyword(
                    keyword @ (crate::program::CounterKeyword::Persist
                    | crate::program::CounterKeyword::Undying),
                ) => {
                    let Some(binding) = self
                        .execution_bindings(entry.id())
                        .and_then(|b| b.trigger.as_ref())
                    else {
                        return Ok(None);
                    };
                    let [cause] = binding.causes.members() else {
                        return Ok(None);
                    };
                    let crate::occurrence::TriggerRelated::ZoneChange { before, after } =
                        &cause.related
                    else {
                        return Ok(None);
                    };
                    if !keyword.eligible_at_death(before) {
                        return Ok(None);
                    }
                    let object = after.object.id();
                    let Some(card) = self.window.cards().get(object) else {
                        return Ok(None);
                    };
                    let owner = card.owner();
                    if card.zone() != ObjectZone::Graveyard(owner) {
                        return Ok(None);
                    }
                    (object, owner)
                }
                _ => return Ok(None),
            }
        } else {
            let Some(object) = self.window.spell_object(entry) else {
                return Ok(None);
            };
            (object, entry.controller())
        };
        if self.resolution_entry_choices.contains_key(&object) {
            return Ok(None);
        }
        self.object_entry_choice_frame(chooser, object)
    }

    fn copied_spell_aura_frame(
        &self,
        entry: &crate::stack_window::StackEntry,
        object: ObjectId,
    ) -> Result<Option<LinkedAuraChoiceFrame>, EffectError> {
        if self.resolution_aura_choices.contains_key(&object)
            || !entry.targets().targets().is_empty()
        {
            return Ok(None);
        }
        let Some(copy) = self.prospective_entry_copies.get(&object) else {
            return Ok(None);
        };
        let chooser = self
            .prospective_entry_controllers
            .get(&object)
            .copied()
            .unwrap_or(entry.controller());
        Ok(self
            .token_aura_domain(
                &copy.definition,
                chooser,
                &self.evaluated_characteristics()?,
            )
            .map(|domain| LinkedAuraChoiceFrame {
                revision: self.revision,
                chooser,
                object,
                domain,
            }))
    }
    fn graveyard_aura_entry_frame(&self) -> Result<Option<LinkedAuraChoiceFrame>, EffectError> {
        if !matches!(
            self.current_instruction()?,
            Instruction::ReturnOwnGraveyardCreatureToBattlefield
                | Instruction::ReturnOwnGraveyardPermanentToBattlefield { .. }
                | Instruction::ReturnAnyGraveyardCreatureBlackSubtype(_)
                | Instruction::ReturnAnyGraveyardCreatureOrPlaneswalker
        ) {
            return Ok(None);
        }
        let entry = self.window.resolving().ok_or(EffectError::WrongPhase)?;
        let TargetRef::Object(object) = self.instruction_target(entry)?.choice().target else {
            return Ok(None);
        };
        if self.resolution_aura_choices.contains_key(&object) {
            return Ok(None);
        }
        let evaluated = self.evaluated_characteristics()?;
        let chooser = self
            .prospective_entry_controllers
            .get(&object)
            .copied()
            .unwrap_or(entry.controller());
        let Some(domain) = self.linked_aura_domain(object, chooser, &evaluated) else {
            return Ok(None);
        };
        Ok(Some(LinkedAuraChoiceFrame {
            revision: self.revision,
            chooser,
            object,
            domain,
        }))
    }
    fn stage_resolution_entry_choice(
        &mut self,
        old: ObjectId,
        new: ObjectId,
    ) -> Result<(), EffectError> {
        if let Some(copy) = self.prospective_entry_copies.remove(&old) {
            self.characteristics
                .entry_copies
                .insert(new, copy.definition);
            if copy.extra_counters > 0 {
                self.copy_entry_counters.insert(new, copy.extra_counters);
            }
        }
        if let Some(controller) = self.prospective_entry_controllers.remove(&old) {
            if self
                .copiable_program(new)
                .and_then(Program::permanent)
                .is_some_and(|p| p.entry_tap == Some(crate::land::EntryTap::ChosenOpponentControls))
            {
                self.entry_choices
                    .insert(new, crate::land::EntryChoice::Opponent(controller));
                self.characteristics
                    .entry_player_choices
                    .insert(new, controller);
            } else {
                self.window.set_entering_controller(new, controller)?;
            }
        }
        if let Some(count) = self.prospective_devour_counters.remove(&old) {
            self.prospective_devour_counters.insert(new, count);
        }
        if let Some(choice) = self.resolution_entry_choices.remove(&old) {
            match choice {
                crate::land::EntryChoice::Color(color) => {
                    self.entry_color_choices.insert(new, color);
                    if let Some(c) = color.color() {
                        self.characteristics.entry_color_choices.insert(new, c);
                    }
                }
                reveal @ (crate::land::EntryChoice::Players(_)
                | crate::land::EntryChoice::Opponent(_)
                | crate::land::EntryChoice::Reveal(_)
                | crate::land::EntryChoice::Chapter(_)
                | crate::land::EntryChoice::CardName(_)
                | crate::land::EntryChoice::PayLife(_)
                | crate::land::EntryChoice::OptionIndex(_)) => {
                    if let crate::land::EntryChoice::OptionIndex(idx) = reveal {
                        self.characteristics.entry_option_choices.insert(new, idx);
                    } else if let crate::land::EntryChoice::Opponent(seat) = reveal {
                        self.characteristics.entry_player_choices.insert(new, seat);
                    }
                    self.entry_choices.insert(new, reveal);
                }
                crate::land::EntryChoice::CreatureType(kind) => {
                    self.entry_creature_type_choices.insert(new, kind);
                }
            }
        }
        Ok(())
    }
    fn begin_battlefield_entry_replacement(
        &mut self,
        object: ObjectId,
        controller: SeatId,
        reason: crate::occurrence::ZoneReason,
    ) -> Result<(), EffectError> {
        // CR 800.4b leaves the object in its current zone before any entry replacements.
        if !self.window.participants().contains(&controller) {
            return self.advance_instruction();
        }
        let cursor = self
            .resolution_cursor()
            .ok_or(EffectError::WrongResolutionFrame)?
            .clone();
        let batch = self.begin_zone_replacements_with_entry(
            zone_replacement::AuthorizedZoneBatch {
                cause: crate::occurrence::EventCause::Instruction {
                    entry: cursor.entry,
                    instruction: cursor.instruction,
                },
                action: crate::program::ZoneActionKind::EffectEnterBattlefield,
                intents: vec![zone_replacement::ZoneMoveIntent {
                    before: object,
                    reason,
                }],
            },
            Some(&BTreeMap::from([(object, controller)])),
        )?;
        let Some(PendingWork::Resolution(cursor)) = self.pending_work.take() else {
            return Err(EffectError::WrongResolutionFrame);
        };
        self.pending_work = Some(PendingWork::BattlefieldEntry(Box::new(
            BattlefieldEntryWork {
                cursor,
                object,
                controller,
                reason,
                action: crate::program::ZoneActionKind::EffectEnterBattlefield,
                batch,
            },
        )));
        Ok(())
    }
    fn begin_hand_return_replacement(
        &mut self,
        object: ObjectId,
        controller: SeatId,
    ) -> Result<(), EffectError> {
        let cursor = self
            .resolution_cursor()
            .ok_or(EffectError::WrongResolutionFrame)?
            .clone();
        let action = crate::program::ZoneActionKind::EffectReturnSelected;
        let reason = crate::occurrence::ZoneReason::ReturnToOwnersHand;
        let batch = self.begin_zone_replacements(zone_replacement::AuthorizedZoneBatch {
            cause: crate::occurrence::EventCause::Instruction {
                entry: cursor.entry,
                instruction: cursor.instruction,
            },
            action,
            intents: vec![zone_replacement::ZoneMoveIntent {
                before: object,
                reason,
            }],
        })?;
        let Some(PendingWork::Resolution(cursor)) = self.pending_work.take() else {
            return Err(EffectError::WrongResolutionFrame);
        };
        self.pending_work = Some(PendingWork::BattlefieldEntry(Box::new(
            BattlefieldEntryWork {
                cursor,
                object,
                controller,
                reason,
                action,
                batch,
            },
        )));
        Ok(())
    }
    fn progress_battlefield_entry(
        &mut self,
        append: &mut RulesJournal,
    ) -> Result<Option<EffectDecision>, EffectError> {
        let Some(PendingWork::BattlefieldEntry(mut work)) = self.pending_work.take() else {
            return Err(EffectError::WrongResolutionFrame);
        };
        if let Some(frame) = work.batch.progress(self.revision)? {
            self.pending_work = Some(PendingWork::BattlefieldEntry(work));
            return Ok(Some(frame));
        }
        let input = zone_replacement::AuthorizedZoneBatch {
            cause: crate::occurrence::EventCause::Instruction {
                entry: work.cursor.entry,
                instruction: work.cursor.instruction,
            },
            action: work.action,
            intents: vec![zone_replacement::ZoneMoveIntent {
                before: work.object,
                reason: work.reason,
            }],
        };
        let fresh = if work.action == crate::program::ZoneActionKind::EffectEnterBattlefield {
            self.begin_zone_replacements_with_entry(
                input,
                Some(&BTreeMap::from([(work.object, work.controller)])),
            )?
        } else {
            self.begin_zone_replacements(input)?
        };
        work.batch.verify_initial(fresh)?;
        self.prospective_entry_copies
            .extend(work.batch.entry_copies());
        self.prospective_entry_controllers
            .extend(work.batch.entry_controllers());
        let destination = work.batch.destination(work.object)?;
        work.batch.commit_devour(self, append)?;
        self.settled_entry_destinations
            .insert(work.object, destination);
        self.pending_work = Some(PendingWork::Resolution(work.cursor));
        Ok(None)
    }
    fn advance_instruction(&mut self) -> Result<(), EffectError> {
        self.draw_replacement = None;
        let cursor = self
            .resolution_cursor_mut()
            .ok_or(EffectError::WrongResolutionFrame)?;
        cursor.results.sampled_graveyard_cards = None;
        cursor.chosen_damage_recipient = None;
        cursor.instruction = cursor
            .instruction
            .checked_add(1)
            .ok_or(EffectError::InconsistentProgram)?;
        Ok(())
    }
    fn copy_resolution_frame(&self) -> Result<CopyResolution, EffectError> {
        let entry = self.window.resolving().ok_or(EffectError::WrongPhase)?;
        let original = if self.current_instruction()? == &Instruction::CopyCapturedAbility {
            self.delayed_ability_source(entry.id())?.original()
        } else if matches!(
            self.current_instruction()?,
            Instruction::CopyForOtherCreatures
                | Instruction::CopyResolvingSpell
                | Instruction::CopyOnCast(_)
        ) {
            self.copying_original(entry.id())
                .ok_or(EffectError::WrongResolutionFrame)?
        } else if matches!(
            self.current_instruction()?,
            Instruction::CopyAbility
                | Instruction::CopySpell
                | Instruction::ChooseNewTargets
                | Instruction::ChangeOnlyTarget
        ) {
            let bound = self.instruction_target(entry)?;
            let TargetRef::Stack(original) = bound.choice().target else {
                return Err(EffectError::InconsistentProgram);
            };
            original
        } else {
            return Err(EffectError::WrongResolutionFrame);
        };
        Ok(CopyResolution {
            revision: self.revision,
            entry: entry.id(),
            original,
            target_ordinal: match self.payloads.get(&entry.id()) {
                Some(StackPayload::Instructions(payload)) => self
                    .copy_creature_order_len(entry.id())
                    .unwrap_or(payload.pending_copy_targets.len()),
                _ => return Err(EffectError::WrongResolutionFrame),
            },
            chooser: self.copy_chooser(entry.id()).unwrap_or(entry.controller()),
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
    fn continue_linked_returns(
        mut self,
        turn: Option<&TurnScheduler>,
    ) -> Result<RuleProgress, EffectError> {
        let mut append = RulesJournal::default();
        self.finish_linked_return_if_ready(&mut append)?;
        if let Some(frame) = self.linked_return_decision()? {
            return Ok(RuleProgress::new(
                self,
                RuleResult::Pending {
                    checks: CheckJournal::default(),
                    frame,
                    journal: DamageJournal::default(),
                    declarations: vec![],
                    assignments: vec![],
                    draws: DrawJournal::default(),
                },
                append,
            ));
        }
        if let Some(mut cast) = self.deferred_cast.take() {
            cast.resources =
                self.pay_cast_resources(&cast.resources, cast.entry, turn, &mut append)?;
            if self.has_pending_cost_or_linked() {
                self.deferred_cast = Some(cast);
                let frame = if let Some(frame) = self.pending_cast_cost_decision() {
                    frame
                } else {
                    self.linked_return_decision()?
                        .ok_or(EffectError::InconsistentProgram)?
                };
                return Ok(RuleProgress::new(
                    self,
                    RuleResult::Pending {
                        checks: CheckJournal::default(),
                        frame,
                        journal: DamageJournal::default(),
                        declarations: vec![],
                        assignments: vec![],
                        draws: DrawJournal::default(),
                    },
                    append,
                ));
            }
            self.debit_mana_at_cut(&cast.receipt, true, turn, &mut append)?;
            self.completed_deferred_receipts = Some((cast.receipt.clone(), None));
            let copy_source = if cast.copy_source_needed {
                let prior = self
                    .cast_history
                    .as_ref()
                    .map_or_else(|| 0.into(), |history| history.spells.clone());
                Some(Box::new(self.capture_copy_source(cast.entry, prior)?))
            } else {
                None
            };
            let completed = self.rules_snapshot(turn)?;
            self.emit_from(
                crate::occurrence::EventCause::CastCompleted { entry: cast.entry },
                &completed,
                turn,
                &mut append,
                |_, current, _| {
                    let mut value = current.cast_occurrence(
                        cast.entry,
                        cast.actor,
                        &cast.receipt.spend,
                        cast.mana_value,
                    )?;
                    value.copy_source = copy_source;
                    Ok(vec![crate::occurrence::RulesOccurrence::CastCompleted(
                        Box::new(value),
                    )])
                },
            )?;
        }
        if let Some((owner, receipt)) = self.deferred_selected_activation.take() {
            self.finish_selected_activation(owner, receipt, &mut append)?;
        }
        if let Some(deferred) = self.deferred_activation.take() {
            self.debit_mana_at_cut(&deferred.receipt, false, None, &mut append)?;
            self.completed_deferred_receipts =
                Some((deferred.receipt.clone(), Some(deferred.activation.clone())));
            let complete = self.rules_snapshot(None)?;
            let activation = deferred.activation;
            self.emit_from(
                crate::occurrence::EventCause::ActivatedAbilityCompleted {
                    entry: activation.entry,
                    ability: activation.ability,
                },
                &complete,
                None,
                &mut append,
                |_, _, _| {
                    Ok(vec![crate::occurrence::RulesOccurrence::ActivatedAbility {
                        entry: activation.entry,
                        source: activation.source,
                        ability: activation.ability,
                    }])
                },
            )?;
        }
        if matches!(self.pending_work, Some(PendingWork::Checks(_))) {
            return self.progress_checks(CheckJournal::default(), append, turn);
        }
        if let Some(frame) = self.pending_resolution_decision() {
            return Ok(RuleProgress::new(
                self,
                RuleResult::Pending {
                    checks: CheckJournal::default(),
                    frame,
                    journal: DamageJournal::default(),
                    declarations: vec![],
                    assignments: vec![],
                    draws: DrawJournal::default(),
                },
                append,
            ));
        }
        if self.pending_work.is_some() {
            return self.progress_rules(
                DamageJournal::default(),
                vec![],
                vec![],
                DrawJournal::default(),
                append,
            );
        }
        Ok(RuleProgress::new(
            self,
            RuleResult::Complete {
                checks: CheckJournal::default(),
                completion: RuleCompletion::LinkedReturn,
                journal: DamageJournal::default(),
                declarations: vec![],
                assignments: vec![],
                permanent_entry: None,
                draws: DrawJournal::default(),
            },
            append,
        ))
    }
    fn finish_mana_continuation(
        mut self,
        mut journal: DamageJournal,
        declarations: Vec<CombatDeclarationRecord>,
        assignments: Vec<CombatAssignmentRecord>,
        draws: DrawJournal,
        mut append: RulesJournal,
    ) -> Result<RuleProgress, EffectError> {
        if self.settle_departed_payer(&mut append)? {
            return self.progress_rules(journal, declarations, assignments, draws, append);
        }
        if self.is_paying_attack_cost()
            || matches!(self.pending_work, Some(PendingWork::CastRecovery(_)))
        {
            let frame = self
                .pending_resolution_decision()
                .ok_or(EffectError::InconsistentProgram)?;
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
        if let Some(mut deferred) = self.deferred_mana_cast.take() {
            let mut interrupted = None;
            let mut reselect = false;
            while let Some(activation) = deferred.remaining.first().cloned() {
                let mut candidate = self.clone();
                let mut produced = RulesJournal::default();
                let cast = ResolvingCast {
                    parent: deferred.parent_entry,
                    kind: deferred.kind,
                    mana_abilities: std::slice::from_ref(&activation),
                };
                match candidate.activate_cast_mana(cast, deferred.prepared.actor, &mut produced) {
                    Ok(damage) => {
                        self = candidate;
                        append.append(produced);
                        deferred.remaining.remove(0);
                        if let Some((_, more_damage)) = damage {
                            interrupted = Some(more_damage);
                            break;
                        }
                    }
                    Err(
                        EffectError::UnknownObject
                        | EffectError::NotManaSource
                        | EffectError::UnknownManaAbility
                        | EffectError::CreatureTapRestricted
                        | EffectError::ManaActivationConditionNotMet
                        | EffectError::IllegalManaColor
                        | EffectError::WrongManaColorFrame
                        | EffectError::IllegalManaBundle
                        | EffectError::IllegalHybridManaPayment
                        | EffectError::InsufficientMana
                        | EffectError::ManaPaymentRequired
                        | EffectError::InvalidManaPayment(_)
                        | EffectError::Window(
                            StackWindowError::SourceTapped | StackWindowError::WrongController,
                        ),
                    ) => {
                        deferred.remaining.clear();
                        reselect = true;
                        break;
                    }
                    Err(error) => return Err(error),
                }
            }
            if let Some(more_damage) = interrupted {
                journal.append(more_damage);
                self.deferred_mana_cast = Some(deferred);
                return self.progress_rules(journal, declarations, assignments, draws, append);
            }
            let life_cost = deferred
                .prepared
                .life_payment
                .clone()
                .unwrap_or_else(|| 0.into());
            let life_unpayable = life_cost.is_positive()
                && self
                    .life
                    .get(deferred.prepared.actor)
                    .is_none_or(|total| total < &life_cost);
            let mana_unpayable = match self.prepare_cast_mana_payment(
                &deferred.prepared.unpaid_mana,
                deferred.prepared.payment.mana.as_ref(),
                deferred.prepared.actor,
                deferred.prepared.entry,
                &deferred.prepared.payment.restricted_mana,
            ) {
                Ok(_) => false,
                Err(
                    EffectError::InsufficientMana
                    | EffectError::ManaPaymentRequired
                    | EffectError::InvalidManaPayment(_),
                ) => true,
                Err(error) => return Err(error),
            };
            if reselect || life_unpayable || mana_unpayable {
                let spell = match self
                    .window
                    .entries()
                    .iter()
                    .find(|entry| entry.id() == deferred.prepared.entry)
                    .map(|entry| entry.kind())
                {
                    Some(StackKind::CardSpell(object)) => *object,
                    _ => return Err(EffectError::InconsistentProgram),
                };
                let frame = CastRecoveryFrame {
                    revision: self.revision,
                    chooser: deferred.prepared.actor,
                    spell,
                    life_cost,
                    mana_cost: deferred.prepared.unpaid_mana.clone(),
                };
                self.deferred_mana_cast = Some(deferred);
                self.pending_work = Some(PendingWork::CastRecovery(Box::new(frame.clone())));
                return Ok(RuleProgress::new(
                    self,
                    RuleResult::Pending {
                        checks: CheckJournal::default(),
                        frame: EffectDecision::CastRecovery(frame),
                        journal,
                        declarations,
                        assignments,
                        draws,
                    },
                    append,
                ));
            }
            let mut cursor = deferred
                .parent_cursor
                .take()
                .ok_or(EffectError::InconsistentProgram)?;
            if matches!(
                deferred.kind,
                ExileCastAbility::RevealedLibrary | ExileCastAbility::Impulse
            ) {
                cursor.impulse.decided = true;
                cursor.impulse.cast = true;
            }
            self.pending_work = Some(PendingWork::Resolution(cursor));
            let (next, receipt, cast_append) = self
                .finish_cast_payment(
                    deferred.prepared,
                    deferred.turn.as_ref(),
                    RulesJournal::default(),
                )?
                .into_parts();
            self = next;
            append.append(cast_append);
            if self.deferred_cast.is_none() {
                self.completed_deferred_receipts = Some((receipt, None));
            }
            return self.progress_rules(journal, declarations, assignments, draws, append);
        }
        Ok(RuleProgress::new(
            self,
            RuleResult::Complete {
                checks: CheckJournal::default(),
                completion: RuleCompletion::ManaAbility,
                journal,
                declarations,
                assignments,
                permanent_entry: None,
                draws,
            },
            append,
        ))
    }
    fn entry_exit(
        &self,
        entry: &crate::stack_window::StackEntry,
        resolved: bool,
    ) -> Option<ObjectDestination> {
        let rebound_selected = matches!(self.payloads.get(&entry.id()),Some(StackPayload::Instructions(payload)) if payload.rebound_over_buyback==Some(true));
        let self_exile = resolved
            && matches!(self.payloads.get(&entry.id()), Some(StackPayload::Instructions(payload)) if payload.exile_on_resolve);
        let self_return_to_hand = resolved
            && matches!(self.payloads.get(&entry.id()), Some(StackPayload::Instructions(payload)) if payload.return_to_hand_on_resolve);
        let self_library = if resolved {
            match self.payloads.get(&entry.id()) {
                Some(StackPayload::Instructions(payload)) => payload.library_on_resolve,
                _ => None,
            }
        } else {
            None
        };
        let temp_exile_spell = self.temporary_rules.iter().any(|active| {
            let crate::program::TemporaryRuleKind::ExileSpellOnResolve { filter, .. } =
                &active.rule
            else {
                return false;
            };
            if !active.resolved_players.contains(&entry.controller()) {
                return false;
            }
            match filter {
                None => true,
                Some(f) => match entry.kind() {
                    StackKind::CardSpell(object) => {
                        self.evaluated_characteristics().ok().is_some_and(|eval| {
                            eval.get(*object).is_some_and(|view| {
                                self.matches_object_filter(
                                    *f,
                                    view,
                                    active.controller,
                                    active.source_object,
                                    &BTreeSet::new(),
                                    &eval,
                                )
                            })
                        })
                    }
                    _ => true,
                },
            }
        });
        let destination = if self_exile || temp_exile_spell {
            ObjectDestination::Exile
        } else if let Some(placement) = self_library {
            match placement {
                crate::program::LibraryPlacement::Top => ObjectDestination::LibraryTop,
                crate::program::LibraryPlacement::Bottom
                | crate::program::LibraryPlacement::BottomRandom
                | crate::program::LibraryPlacement::ChoiceTopOrBottom { .. } => {
                    ObjectDestination::LibraryBottom
                }
                crate::program::LibraryPlacement::FromTop(offset) => {
                    ObjectDestination::LibraryFromTop(offset)
                }
            }
        } else if self_return_to_hand || (resolved && entry.bought_back() && !rebound_selected) {
            ObjectDestination::Hand
        } else {
            ObjectDestination::Graveyard
        };
        match entry.kind() {
            StackKind::CardSpell(object) => self.window.cards().get(*object).map(|_| destination),
            StackKind::SpellCopy { .. } => self
                .window
                .spell_copies()
                .contains_key(&entry.id())
                .then_some(destination),
            StackKind::Speed
            | StackKind::Monarch
            | StackKind::DelayedCopy { .. }
            | StackKind::Emblem { .. }
            | StackKind::ActivatedAbility { .. }
            | StackKind::TriggeredAbility { .. } => None,
        }
    }
    fn existing_entry_target_legal(
        &self,
        entry: &crate::stack_window::StackEntry,
        choice: TargetChoice,
        current: &EvaluatedCharacteristics,
        source: &protection::ProtectionSource,
    ) -> Result<bool, EffectError> {
        self.existing_entry_target_legal_with_primary(entry, choice, current, source, None)
    }

    fn existing_entry_target_legal_with_primary(
        &self,
        entry: &crate::stack_window::StackEntry,
        choice: TargetChoice,
        current: &EvaluatedCharacteristics,
        source: &protection::ProtectionSource,
        primary: Option<TargetRef>,
    ) -> Result<bool, EffectError> {
        let target = choice.target;
        if let Some(StackPayload::Instructions(payload)) = self.payloads.get(&entry.id())
            && let Some(required) = &payload.spell_target_mana_value
            && !matches!(target,TargetRef::Stack(id) if self.entry_spell_mana_value(id).as_ref()==Some(required))
        {
            return Ok(false);
        }
        if target == TargetRef::Stack(entry.id()) || Self::protected_target(target, source, current)
        {
            return Ok(false);
        }
        if let Some(dependent) = self
            .stack_body(entry.id())
            .and_then(ExecutableBody::dependent_targets)
            && choice.clause == TargetClauseId(1)
        {
            let primary = primary
                .or_else(|| {
                    entry
                        .targets()
                        .targets()
                        .first()
                        .map(|row| row.choice().target)
                })
                .ok_or(EffectError::InconsistentProgram)?;
            return Ok(self.legal_dependent_target(dependent, primary, target, current));
        }
        if let Some(StackPayload::Instructions(payload)) = self.payloads.get(&entry.id())
            && let Some(seat) = payload.target_controllers.get(&choice.id)
            && !matches!(target, TargetRef::Object(id) if current.get(id).and_then(|view| view.controller) == Some(*seat))
        {
            return Ok(false);
        }
        let entry_source = match entry.kind() {
            StackKind::ActivatedAbility { source } | StackKind::TriggeredAbility { source } => {
                Some(source.id())
            }
            StackKind::CardSpell(id) => Some(*id),
            _ => None,
        };
        let trigger_source_names = self
            .execution_bindings(entry.id())
            .and_then(|b| b.trigger.as_ref())
            .map(|t| &t.source.current_names);
        let source_ctx = TargetSourceContext {
            id: entry_source,
            names: trigger_source_names,
        };
        if let Some(StackPayload::Instructions(payload)) = self.payloads.get(&entry.id())
            && let Some(body) = payload.splice_target_bodies.get(&choice.id)
        {
            return Ok(self.legal_body_target_in_with_source(
                body,
                target,
                entry.controller(),
                source_ctx,
                current,
            ));
        }
        if let Some(kind) = self.mode_target_contract(entry.id(), choice.id) {
            if matches!(kind, BodyTarget::FilteredPermanent(filter) if filter.excludes_ability_source())
                && entry_source.is_some_and(|src| target == TargetRef::Object(src))
            {
                return Ok(false);
            }
            return Ok(self.legal_target_kind_with_source(
                kind,
                target,
                entry.controller(),
                source_ctx,
                current,
            ));
        }
        if let Some(spell) = self.mutating_spell(entry) {
            let owner = self
                .window
                .cards()
                .get(spell)
                .ok_or(EffectError::InconsistentProgram)?
                .owner();
            return Ok(self.legal_mutation_target(owner, target, current));
        }
        if let Some(enchant) = self.aura_on_stack(entry) {
            return Ok(self.aura_target_legal(enchant, target, entry.controller(), current));
        }
        let body = self
            .stack_body(entry.id())
            .ok_or(EffectError::InconsistentProgram)?;
        if body.target == same_name_ability_target()
            && !matches!(entry.kind(), StackKind::ActivatedAbility { source }
                if self.target_shares_ability_source_name(source.id(), target, current))
            && !matches!(entry.kind(), StackKind::TriggeredAbility { .. })
        {
            return Ok(false);
        }
        if matches!(body.target, BodyTarget::FilteredPermanent(filter) if filter.excludes_ability_source())
            && matches!((target,entry.kind()),(TargetRef::Object(id), StackKind::ActivatedAbility {source} | StackKind::TriggeredAbility {source}) if source.id() == id)
        {
            return Ok(false);
        }
        let source = self
            .execution_bindings(entry.id())
            .and_then(|b| b.trigger.as_ref())
            .map(|t| &t.source);
        Ok(self.mentor_target(body, target, source, current)
            && self.legal_body_target_in_with_source(
                body,
                target,
                entry.controller(),
                source_ctx,
                current,
            ))
    }
    fn legal_dependent_target(
        &self,
        relation: crate::program::DependentTargets,
        primary: TargetRef,
        candidate: TargetRef,
        current: &EvaluatedCharacteristics,
    ) -> bool {
        let TargetRef::Object(id) = candidate else {
            return false;
        };
        match relation {
            crate::program::DependentTargets::CreatureControlledByPrimary => self
                .related_target_controller(primary, current)
                .is_some_and(|controller| {
                    current.get(id).is_some_and(|creature| {
                        creature.zone == ObjectZone::Battlefield
                            && creature.is_creature()
                            && creature.controller == Some(controller)
                    })
                }),
            crate::program::DependentTargets::PrimaryPlayersGraveyard { .. } => {
                matches!(primary, TargetRef::Player(player) if self.window.cards().get(id).is_some_and(|card|
                    card.card_id().is_some() && card.zone() == ObjectZone::Graveyard(player)))
            }
        }
    }

    fn related_target_controller(
        &self,
        target: TargetRef,
        current: &EvaluatedCharacteristics,
    ) -> Option<SeatId> {
        match target {
            TargetRef::Player(player) => Some(player),
            TargetRef::Object(id) => current
                .get(id)
                .filter(|object| object.zone == ObjectZone::Battlefield)
                .and_then(|object| object.controller)
                .or_else(|| {
                    self.growth_last_known
                        .get(&id)
                        .and_then(|object| object.object.controller())
                }),
            TargetRef::Stack(_) => None,
        }
    }

    fn target_shares_ability_source_name(
        &self,
        source: ObjectId,
        target: TargetRef,
        current: &EvaluatedCharacteristics,
    ) -> bool {
        let TargetRef::Object(target) = target else {
            return false;
        };
        let source_names = current
            .get(source)
            .filter(|source| source.zone == ObjectZone::Battlefield)
            .map(|source| &source.current_names)
            .or_else(|| {
                self.growth_last_known
                    .get(&source)
                    .map(|source| &source.current_names)
            });
        source_names.is_some_and(|source_names| {
            current.get(target).is_some_and(|target| {
                self.name_rules
                    .shares_name(source_names, &target.current_names)
            })
        })
    }

    #[cfg(test)]
    fn legal_body_target(
        &self,
        body: &ExecutableBody,
        target: TargetRef,
        controller: SeatId,
    ) -> Result<bool, EffectError> {
        let evaluated = self.evaluated_characteristics()?;
        Ok(self.legal_body_target_in(body, target, controller, &evaluated))
    }
    fn stack_entry_targets_controlled_permanent(
        &self,
        target: TargetRef,
        controller: SeatId,
        current: &EvaluatedCharacteristics,
        minimum_power: Option<u8>,
    ) -> bool {
        let TargetRef::Stack(id) = target else {
            return false;
        };
        self.window
            .entries()
            .iter()
            .find(|entry| entry.id() == id)
            .is_some_and(|entry| {
                entry.targets().targets().iter().any(|target| {
                    let TargetRef::Object(object) = target.choice().target else {
                        return false;
                    };
                    current.get(object).is_some_and(|p| {
                        p.zone == ObjectZone::Battlefield
                            && p.controller == Some(controller)
                            && minimum_power.is_none_or(|power| {
                                p.is_creature()
                                    && p.power_toughness.as_ref().is_some_and(|stats| {
                                        stats.derived.power >= i64::from(power).into()
                                    })
                            })
                    })
                })
            })
    }
    fn legal_body_target_in(
        &self,
        body: &ExecutableBody,
        target: TargetRef,
        controller: SeatId,
        evaluated: &EvaluatedCharacteristics,
    ) -> bool {
        self.legal_body_target_in_with_source(
            body,
            target,
            controller,
            TargetSourceContext::default(),
            evaluated,
        )
    }

    pub(super) fn legal_body_target_in_with_source(
        &self,
        body: &ExecutableBody,
        target: TargetRef,
        controller: SeatId,
        source: TargetSourceContext<'_>,
        evaluated: &EvaluatedCharacteristics,
    ) -> bool {
        if let [Instruction::ReturnGraveyardCreatureWithCorpse(mana_value)] =
            body.instructions.as_slice()
            && !matches!(target,TargetRef::Object(id) if self.copiable_program(id).is_some_and(|p|p.mana_value()==*mana_value))
        {
            return false;
        }
        if let [_, Instruction::ResolveTransfer(key)] = body.instructions.as_slice()
            && !self.transfer_target(*key, target, evaluated)
        {
            return false;
        }
        if let [Instruction::Attach(ability)] = body.instructions.as_slice()
            && !matches!(target, TargetRef::Object(id) if evaluated.get(id).is_some_and(|object| object.controller == Some(controller) && object.has_type(ability.host_type())))
        {
            return false;
        }
        self.legal_target_kind_with_source(body.target, target, controller, source, evaluated)
    }

    fn ability_source_names<'a>(
        &'a self,
        source: ObjectId,
        evaluated: &'a EvaluatedCharacteristics,
    ) -> Option<&'a spellwake_contracts::names::ObjectNames> {
        evaluated
            .get(source)
            .map(|view| &view.current_names)
            .or_else(|| {
                self.growth_last_known
                    .get(&source)
                    .map(|appearance| &appearance.current_names)
            })
            .or_else(|| {
                self.departed_reflexive_sources
                    .get(&source)
                    .map(|appearance| &appearance.current_names)
            })
            .or_else(|| {
                self.window
                    .resolving()
                    .and_then(|entry| self.execution_bindings(entry.id()))
                    .and_then(|bindings| bindings.trigger.as_ref())
                    .filter(|trigger| trigger.source.object.id() == source)
                    .map(|trigger| &trigger.source.current_names)
            })
            .or_else(|| self.copiable_program(source).map(|program| &program.names))
    }

    fn matches_object_clause_with_source(
        &self,
        clause: crate::program::ObjectClause,
        p: &ObjectCharacteristics,
        controller: SeatId,
        source: TargetSourceContext<'_>,
        excluded_subtypes: &BTreeSet<CreatureTypeId>,
        evaluated: &EvaluatedCharacteristics,
    ) -> bool {
        use crate::program::{
            CombatFilter, ControllerFilter, ObjectFilterFlags, PowerFilter, SourceRelation,
        };
        let ability_source = source.id;
        let id = p.object;
        let controller_ok = match clause.controller {
            ControllerFilter::Any => true,
            ControllerFilter::You => p.controller == Some(controller),
            ControllerFilter::Opponent => p.controller.is_some_and(|seat| {
                self.combat_rules
                    .opponents(self.window.participants(), controller)
                    .is_ok_and(|opponents| opponents.contains(&seat))
            }),
            ControllerFilter::NotYou => p.controller != Some(controller),
            ControllerFilter::OwnerYou => self
                .window
                .cards()
                .get(id)
                .is_some_and(|object| object.owner() == controller),
            ControllerFilter::OwnerOpponent => self.window.cards().get(id).is_some_and(|object| {
                self.combat_rules
                    .opponents(self.window.participants(), controller)
                    .is_ok_and(|opponents| opponents.contains(&object.owner()))
            }),
            ControllerFilter::RelatedPlayer | ControllerFilter::FirstTargetController => false,
            ControllerFilter::TargetPlayer | ControllerFilter::TargetOpponent => self
                .resolving_player(crate::program::ResolvingPlayer::TargetPlayer)
                .is_ok_and(|seat| p.controller == Some(seat)),
            ControllerFilter::ActivePlayer => p.controller == Some(self.window.active_player()),
            ControllerFilter::DefendingPlayer => self
                .resolving_player(crate::program::ResolvingPlayer::DefendingPlayer)
                .is_ok_and(|seat| p.controller == Some(seat)),
            ControllerFilter::Player(seat) => p.controller == Some(seat),
        };
        if !controller_ok {
            return false;
        }
        let owner_ok = match clause.owner {
            ControllerFilter::Any => true,
            ControllerFilter::You | ControllerFilter::OwnerYou => p.owner == controller,
            ControllerFilter::Opponent | ControllerFilter::OwnerOpponent => self
                .combat_rules
                .opponents(self.window.participants(), controller)
                .is_ok_and(|opponents| opponents.contains(&p.owner)),
            ControllerFilter::NotYou => p.owner != controller,
            ControllerFilter::RelatedPlayer | ControllerFilter::FirstTargetController => false,
            ControllerFilter::TargetPlayer | ControllerFilter::TargetOpponent => self
                .resolving_player(crate::program::ResolvingPlayer::TargetPlayer)
                .is_ok_and(|seat| p.owner == seat),
            ControllerFilter::ActivePlayer => p.owner == self.window.active_player(),
            ControllerFilter::DefendingPlayer => self
                .resolving_player(crate::program::ResolvingPlayer::DefendingPlayer)
                .is_ok_and(|seat| p.owner == seat),
            ControllerFilter::Player(seat) => p.owner == seat,
        };
        if !owner_ok {
            return false;
        }
        let source_ok = match clause.source_relation {
            SourceRelation::Any | SourceRelation::MatchesNamedAgendaSlot => true,
            SourceRelation::SharingNameWithAbilitySource => ability_source
                .and_then(|src| self.ability_source_names(src, evaluated))
                .or(source.names)
                .is_some_and(|src_names| self.name_rules.shares_name(&p.current_names, src_names)),
            SourceRelation::SelfObject => ability_source == Some(id),
            SourceRelation::AttachedToSelf => ability_source.is_some_and(|src| {
                self.characteristics.attachments.get(&src)
                    == Some(&crate::targets::TargetRef::Object(id))
            }),
            SourceRelation::OtherThanAbilitySource => ability_source.is_none_or(|src| id != src),
            SourceRelation::OtherThanTriggerSource => false,
            SourceRelation::OtherThanBound(src) => id != src,
        };
        if !source_ok {
            return false;
        }
        let combat_ok = match clause.combat {
            CombatFilter::Any => true,
            CombatFilter::Attacking => self
                .combat
                .as_ref()
                .is_some_and(|combat| combat.attacker(id).is_some()),
            CombatFilter::Blocking => self
                .combat
                .as_ref()
                .is_some_and(|combat| combat.blocker(id).is_some()),
            CombatFilter::AttackingOrBlocking => self.combat.as_ref().is_some_and(|combat| {
                combat.attacker(id).is_some() || combat.blocker(id).is_some()
            }),
            CombatFilter::Blocked => self.combat.as_ref().is_some_and(|combat| {
                combat
                    .attacker(id)
                    .is_some_and(|a| a.block_status() == crate::combat::BlockStatus::Blocked)
            }),
            CombatFilter::Unblocked => self.combat.as_ref().is_some_and(|combat| {
                combat
                    .attacker(id)
                    .is_some_and(|a| a.block_status() == crate::combat::BlockStatus::Unblocked)
            }),
            CombatFilter::BlockingSource => ability_source.is_some_and(|src| {
                self.combat.as_ref().is_some_and(|combat| {
                    combat
                        .blocker(id)
                        .is_some_and(|b| b.attackers().contains(&src))
                })
            }),
            CombatFilter::BlockedBySource => ability_source.is_some_and(|src| {
                self.combat.as_ref().is_some_and(|combat| {
                    combat
                        .blocker(src)
                        .is_some_and(|b| b.attackers().contains(&id))
                })
            }),
            CombatFilter::BlockingOrBlockedBySource => ability_source.is_some_and(|src| {
                self.combat.as_ref().is_some_and(|combat| {
                    combat
                        .blocker(id)
                        .is_some_and(|b| b.attackers().contains(&src))
                        || combat
                            .blocker(src)
                            .is_some_and(|b| b.attackers().contains(&id))
                })
            }),
            CombatFilter::NotAttacking => !self
                .combat
                .as_ref()
                .is_some_and(|combat| combat.attacker(id).is_some()),
        };
        if !combat_ok {
            return false;
        }
        if let Some(required_tapped) = clause.tapped
            && !self
                .window
                .cards()
                .get(id)
                .is_some_and(|o| o.tapped() == Some(required_tapped))
        {
            return false;
        }
        if let Some(required_token) = clause.token
            && !self
                .window
                .cards()
                .get(id)
                .is_some_and(|o| o.is_token() == required_token)
        {
            return false;
        }
        if clause.flags.contains(ObjectFilterFlags::WITHOUT_COUNTERS)
            && self
                .characteristics
                .total_permanent_counters(id)
                .is_positive()
        {
            return false;
        }
        if clause.flags.contains(ObjectFilterFlags::WITH_ANY_COUNTER) && !p.has_any_counter {
            return false;
        }
        if clause
            .flags
            .contains(ObjectFilterFlags::WITH_PLUS_ONE_COUNTER)
            && !p.has_plus_one_counter
        {
            return false;
        }
        if clause.flags.contains(ObjectFilterFlags::EQUIPPED)
            && !evaluated.objects.values().any(|att| {
                att.zone == ObjectZone::Battlefield
                    && att.attached_to == Some(TargetRef::Object(id))
                    && matches!(
                        att.attachment_kind,
                        Some(crate::attachment::AttachmentKind::Equipment)
                    )
            })
        {
            return false;
        }
        if clause.flags.contains(ObjectFilterFlags::ENCHANTED)
            && !evaluated.objects.values().any(|att| {
                att.zone == ObjectZone::Battlefield
                    && att.attached_to == Some(TargetRef::Object(id))
                    && matches!(
                        att.attachment_kind,
                        Some(crate::attachment::AttachmentKind::Aura(_))
                    )
            })
        {
            return false;
        }
        if clause.flags.contains(ObjectFilterFlags::MODIFIED) {
            let modified = p.has_any_counter
                || evaluated.objects.values().any(|att| {
                    att.zone == ObjectZone::Battlefield
                        && att.attached_to == Some(TargetRef::Object(id))
                        && (matches!(
                            att.attachment_kind,
                            Some(crate::attachment::AttachmentKind::Equipment)
                        ) || (matches!(
                            att.attachment_kind,
                            Some(crate::attachment::AttachmentKind::Aura(_))
                        ) && att.controller == p.controller))
                });
            if !modified {
                return false;
            }
        }
        if clause
            .flags
            .contains(ObjectFilterFlags::DEALT_DAMAGE_THIS_TURN)
            && !self
                .battlefield_marked_damage(id)
                .is_some_and(|damage| damage.is_positive())
        {
            return false;
        }
        if clause.flags.contains(ObjectFilterFlags::ATTACKED_THIS_TURN)
            && !self.triggers.attacked_this_turn(id)
        {
            return false;
        }
        if clause
            .flags
            .contains(ObjectFilterFlags::DID_NOT_ATTACK_THIS_TURN)
            && self.triggers.attacked_this_turn(id)
        {
            return false;
        }
        if clause.flags.contains(ObjectFilterFlags::ENTERED_THIS_TURN)
            && id < self.turn_start_incarnation
        {
            return false;
        }
        let is_commander = p.commander_owner.is_some()
            || self
                .window
                .cards()
                .get(id)
                .and_then(GameObject::card_id)
                .is_some_and(|card| self.commanders.contains_key(&card));
        if clause.flags.contains(ObjectFilterFlags::COMMANDER_ONLY) && !is_commander {
            return false;
        }
        if clause.flags.contains(ObjectFilterFlags::NOT_COMMANDER) && is_commander {
            return false;
        }
        if clause.flags.contains(ObjectFilterFlags::FACE_DOWN)
            && !self
                .window
                .cards()
                .get(id)
                .is_some_and(|card| card.face_down().is_some())
        {
            return false;
        }
        if clause.flags.contains(ObjectFilterFlags::EXILED_WITH_SELF)
            && !ability_source.is_some_and(|src| {
                self.characteristics
                    .imprinted_cards
                    .get(&src)
                    .is_some_and(|list| list.contains(&id))
                    || self
                        .characteristics
                        .ability_donors_in_exile
                        .get(&src)
                        .is_some_and(|list| list.contains(&id))
                    || self
                        .linked_exiles
                        .get(&src)
                        .is_some_and(|list| list.contains(&id))
            })
        {
            return false;
        }
        if clause
            .flags
            .contains(ObjectFilterFlags::DEALT_DAMAGE_BY_SELF_THIS_TURN)
            && !ability_source.is_some_and(|src| {
                self.characteristics
                    .creature_damage_pairs
                    .as_ref()
                    .is_some_and(|pairs| pairs.contains(&(src, id)))
            })
        {
            return false;
        }
        if clause.flags.contains(ObjectFilterFlags::DIED_THIS_TURN)
            && !self.characteristics.died_this_turn.contains(&id)
        {
            return false;
        }
        if clause
            .flags
            .contains(ObjectFilterFlags::MATCHES_SOURCE_ENTRY_COLOR)
            && !ability_source.is_some_and(|src| {
                self.characteristics
                    .entry_color_choices
                    .get(&src)
                    .is_some_and(|color| p.current_colors.contains(color))
            })
        {
            return false;
        }
        if clause
            .flags
            .contains(ObjectFilterFlags::MATCHES_SOURCE_ENTRY_TYPE)
            && !ability_source.is_some_and(|src| {
                self.entry_creature_type_choices
                    .get(&src)
                    .is_some_and(|kind| p.current_creature_subtypes.contains(kind))
            })
        {
            return false;
        }
        if clause
            .flags
            .contains(ObjectFilterFlags::MATCHES_CHOSEN_CARD_TYPE)
            && !self
                .resolution_cursor()
                .and_then(|cursor| cursor.results.chosen_card_type)
                .or_else(|| ability_source.and_then(|src| self.entry_chosen_card_type(src)))
                .is_some_and(|kind| p.has_type(kind))
        {
            return false;
        }
        if clause
            .flags
            .contains(ObjectFilterFlags::MATCHES_CHOSEN_NAME)
            && !self.object_matches_chosen_name(&p.current_names, ability_source)
        {
            return false;
        }
        let eval_anchor = |anchor: crate::program::DynamicStatAnchor| -> Option<RulesInteger> {
            use crate::program::DynamicStatAnchor as A;
            match anchor {
                A::ChosenX => self.window.resolving().and_then(|e| e.chosen_x().cloned()),
                A::SourcePower => ability_source
                    .and_then(|src| evaluated.get(src))
                    .and_then(|o| {
                        o.power_toughness
                            .as_ref()
                            .map(|pt| pt.derived.power.clone())
                    }),
                A::SourceToughness => {
                    ability_source
                        .and_then(|src| evaluated.get(src))
                        .and_then(|o| {
                            o.power_toughness
                                .as_ref()
                                .map(|pt| pt.derived.toughness.clone())
                        })
                }
                A::SourceManaValue => ability_source.and_then(|src| self.permanent_mana_value(src)),
                A::SourceCounters(kind) => ability_source
                    .map(|src| self.characteristics.permanent_counter_count(src, kind)),
                A::ControllerLife => self.life.get(controller).cloned(),
                A::ControllerHandSize => Some(RulesInteger::from_usize(
                    self.window
                        .cards()
                        .iter()
                        .filter(|o| o.zone() == ObjectZone::Hand(controller))
                        .count(),
                )),
                A::ControllerGraveyardCount => Some(RulesInteger::from_usize(
                    self.window.cards().graveyard(controller).len(),
                )),
                A::ControllerControlledCount(ct) => Some(RulesInteger::from_usize(
                    evaluated
                        .objects
                        .values()
                        .filter(|o| {
                            o.zone == ObjectZone::Battlefield
                                && o.controller == Some(controller)
                                && o.has_type(ct)
                        })
                        .count(),
                )),
                A::ControllerSubtypeCount(sub) => Some(RulesInteger::from_usize(
                    evaluated
                        .objects
                        .values()
                        .filter(|o| {
                            o.zone == ObjectZone::Battlefield
                                && o.controller == Some(controller)
                                && o.current_creature_subtypes.contains(&sub)
                        })
                        .count(),
                )),
                A::TriggerSubjectPower => self
                    .window
                    .resolving()
                    .and_then(|e| self.execution_bindings(e.id()))
                    .and_then(|b| b.trigger.as_ref())
                    .and_then(|t| t.causes.members().first())
                    .and_then(|c| c.subject_object_id())
                    .and_then(|tid| evaluated.get(tid))
                    .and_then(|o| {
                        o.power_toughness
                            .as_ref()
                            .map(|pt| pt.derived.power.clone())
                    }),
                A::TriggerSubjectManaValue => self
                    .window
                    .resolving()
                    .and_then(|e| self.execution_bindings(e.id()))
                    .and_then(|b| b.trigger.as_ref())
                    .and_then(|t| t.causes.members().first())
                    .and_then(|c| c.subject_object_id())
                    .and_then(|tid| self.permanent_mana_value(tid)),
                A::TargetPower => self
                    .window
                    .resolving()
                    .and_then(|e| self.instruction_target(e).ok())
                    .and_then(|t| match t.choice().target {
                        TargetRef::Object(tid) => evaluated
                            .get(tid)
                            .and_then(|o| o.power_toughness.as_ref())
                            .map(|pt| pt.derived.power.clone())
                            .or_else(|| {
                                self.resolution_cursor()
                                    .and_then(|c| c.results.target_lki.get(&tid))
                                    .and_then(|lki| lki.power.clone())
                            }),
                        _ => None,
                    }),
            }
        };
        if let PowerFilter::CompareAnchor { op, anchor } = clause.power
            && !p
                .power_toughness
                .as_ref()
                .zip(eval_anchor(anchor))
                .is_some_and(|(stats, bound)| op.evaluate(&stats.derived.power, &bound))
        {
            return false;
        }
        if let crate::program::ToughnessFilter::CompareAnchor { op, anchor } = clause.toughness
            && !p
                .power_toughness
                .as_ref()
                .zip(eval_anchor(anchor))
                .is_some_and(|(stats, bound)| op.evaluate(&stats.derived.toughness, &bound))
        {
            return false;
        }
        if let crate::program::ManaValueFilter::CompareAnchor { op, anchor } = clause.mana_value {
            let mv = if p.zone == ObjectZone::Battlefield {
                self.permanent_mana_value(id)
            } else {
                self.copiable_program(id).map(Program::mana_value)
            };
            if !mv
                .zip(eval_anchor(anchor))
                .is_some_and(|(value, bound)| op.evaluate(&value, &bound))
            {
                return false;
            }
        }
        if matches!(clause.power, PowerFilter::AtMostControlledCreatureCount)
            && !p.power_toughness.as_ref().is_some_and(|stats| {
                let count = evaluated
                    .objects
                    .values()
                    .filter(|creature| {
                        creature.zone == ObjectZone::Battlefield
                            && creature.is_creature()
                            && creature.controller == Some(controller)
                    })
                    .count();
                stats.derived.power <= RulesInteger::from_usize(count)
            })
        {
            return false;
        }
        clause.matches_static_characteristics(p, excluded_subtypes, || {
            if p.zone == ObjectZone::Battlefield {
                self.permanent_mana_value(id)
            } else {
                self.copiable_program(id).map(Program::mana_value)
            }
        })
    }

    pub(super) fn matches_object_filter(
        &self,
        filter: crate::program::ObjectFilter,
        p: &ObjectCharacteristics,
        controller: SeatId,
        ability_source: Option<ObjectId>,
        excluded_subtypes: &BTreeSet<CreatureTypeId>,
        evaluated: &EvaluatedCharacteristics,
    ) -> bool {
        self.matches_object_filter_with_source(
            filter,
            p,
            controller,
            TargetSourceContext {
                id: ability_source,
                names: None,
            },
            excluded_subtypes,
            evaluated,
        )
    }

    fn matches_object_filter_with_source(
        &self,
        filter: crate::program::ObjectFilter,
        p: &ObjectCharacteristics,
        controller: SeatId,
        source: TargetSourceContext<'_>,
        excluded_subtypes: &BTreeSet<CreatureTypeId>,
        evaluated: &EvaluatedCharacteristics,
    ) -> bool {
        match filter {
            crate::program::ObjectFilter::Clause(a) => self.matches_object_clause_with_source(
                a,
                p,
                controller,
                source,
                excluded_subtypes,
                evaluated,
            ),
            crate::program::ObjectFilter::Either(a, b) => {
                self.matches_object_clause_with_source(
                    a,
                    p,
                    controller,
                    source,
                    excluded_subtypes,
                    evaluated,
                ) || self.matches_object_clause_with_source(
                    b,
                    p,
                    controller,
                    source,
                    excluded_subtypes,
                    evaluated,
                )
            }
        }
    }

    fn matches_graveyard_card(
        &self,
        filter: crate::program::ObjectFilter,
        p: &ObjectCharacteristics,
    ) -> bool {
        let excluded = BTreeSet::new();
        filter.matches_static_characteristics(p, &excluded, || {
            self.copiable_program(p.object).map(Program::mana_value)
        })
    }

    fn legal_target_kind_in(
        &self,
        kind: BodyTarget,
        target: TargetRef,
        controller: SeatId,
        evaluated: &EvaluatedCharacteristics,
    ) -> bool {
        self.legal_target_kind_with_source(
            kind,
            target,
            controller,
            TargetSourceContext::default(),
            evaluated,
        )
    }

    fn legal_target_kind_with_source(
        &self,
        kind: BodyTarget,
        target: TargetRef,
        controller: SeatId,
        source: TargetSourceContext<'_>,
        evaluated: &EvaluatedCharacteristics,
    ) -> bool {
        if let TargetRef::Player(player) = target {
            let has_shroud = evaluated.objects.values().any(|perm| {
                perm.zone == ObjectZone::Battlefield
                    && perm.controller == Some(player)
                    && Self::permanent_has_restriction(perm, |rule| {
                        matches!(
                            rule,
                            crate::characteristics::StaticRestriction::ControllerHasShroud
                        )
                    })
            }) || self.temporary_rules.iter().any(|active| {
                active.controller == player
                    && self.active_temporary_static(active, evaluated).is_some_and(|ab| {
                        matches!(
                            ab,
                            crate::characteristics::StaticContinuousAbility::Restriction {
                                rule: crate::characteristics::StaticRestriction::ControllerHasShroud,
                                ..
                            }
                        )
                    })
            }) || evaluated
                .player_granted_keywords
                .get(&player)
                .is_some_and(|kws| kws.contains(&KeywordAbility::Shroud));
            if has_shroud {
                return false;
            }
            if self
                .combat_rules
                .opponents(self.window.participants(), controller)
                .is_ok_and(|opponents| opponents.contains(&player))
                && (evaluated.objects.values().any(|perm| {
                    perm.zone == ObjectZone::Battlefield
                        && perm.controller == Some(player)
                        && Self::permanent_has_restriction(perm, |rule| {
                            matches!(
                                rule,
                                crate::characteristics::StaticRestriction::ControllerHasHexproof
                            )
                        })
                }) || evaluated
                    .player_granted_keywords
                    .get(&player)
                    .is_some_and(|kws| kws.contains(&KeywordAbility::Hexproof))
                    || self.temporary_rules.iter().any(|active| {
                        active.controller == player
                            && self.active_temporary_static(active, evaluated).is_some_and(|ab| {
                                matches!(
                                    ab,
                                    crate::characteristics::StaticContinuousAbility::Restriction {
                                        rule: crate::characteristics::StaticRestriction::ControllerHasHexproof,
                                        ..
                                    }
                                )
                            })
                    }))
            {
                return false;
            }
        }
        if let TargetRef::Object(id) = target
            && let Some(permanent) = evaluated.get(id)
            && permanent.zone == ObjectZone::Battlefield
            && (permanent.has_keyword(KeywordAbility::Shroud)
                || (permanent.has_keyword(KeywordAbility::Hexproof)
                    && !permanent.hexproof_ignored_by.contains(&controller)
                    && permanent
                        .controller
                        .is_some_and(|owner| owner != controller)))
        {
            // Every other player is an opponent in the admitted free-for-all policy.
            return false;
        }
        if let TargetRef::Object(id) = target
            && self
                .window
                .cards()
                .get(id)
                .is_some_and(|object| matches!(object.zone(), ObjectZone::Graveyard(_)))
            && (evaluated.objects.values().any(|permanent| {
                permanent.zone == ObjectZone::Battlefield
                    && Self::permanent_has_restriction(permanent, |rule| {
                        matches!(
                            rule,
                            crate::characteristics::StaticRestriction::PreventGraveyardTargeting
                        )
                    })
            }) || self.temporary_rules.iter().any(|active| {
                self.active_temporary_static(active, evaluated).is_some_and(|ab| {
                    matches!(
                        ab,
                        crate::characteristics::StaticContinuousAbility::Restriction {
                            rule: crate::characteristics::StaticRestriction::PreventGraveyardTargeting,
                            ..
                        }
                    )
                })
            }))
        {
            return false;
        }
        match kind {
            BodyTarget::None => false,
            BodyTarget::FilteredPermanent(filter) => {
                let TargetRef::Object(id) = target else {
                    return false;
                };
                let Some(p) = evaluated
                    .get(id)
                    .filter(|p| p.zone == ObjectZone::Battlefield)
                else {
                    return false;
                };
                let excluded = BTreeSet::new();
                if source.id.is_some() || source.names.is_some() {
                    self.matches_object_filter_with_source(
                        filter, p, controller, source, &excluded, evaluated,
                    )
                } else if BodyTarget::FilteredPermanent(filter) == same_name_ability_target() {
                    // Legacy callers such as ability activation check target_shares_ability_source_name
                    // before delegating to legal_body_target_in without a source context.
                    self.matches_object_filter(
                        crate::program::ObjectFilter::creature(),
                        p,
                        controller,
                        None,
                        &excluded,
                        evaluated,
                    )
                } else {
                    self.matches_object_filter(filter, p, controller, None, &excluded, evaluated)
                }
            }
            BodyTarget::AnyGraveyardCreature => {
                matches!(target, TargetRef::Object(id) if self.window.cards().get(id).is_some_and(|object| object.card_id().is_some() && matches!(object.zone(), ObjectZone::Graveyard(_))) && evaluated.get(id).is_some_and(|object| object.is_creature()))
            }
            BodyTarget::RecentlyDiscardedOrFromLibrary => {
                matches!(target,TargetRef::Object(id) if self.recent_graveyard_cards.as_ref().is_some_and(|cards|cards.contains(&id)) && self.window.cards().get(id).is_some_and(|card|card.card_id().is_some() && matches!(card.zone(),ObjectZone::Graveyard(_))))
            }
            BodyTarget::OwnedExiledFlashbackCard => matches!(target, TargetRef::Object(id)
                if self.window.cards().get(id).is_some_and(|card| {
                    card.card_id().is_some() && card.owner() == controller && card.zone() == ObjectZone::Exile
                }) && self.face_down_exile_viewer(id).is_none()
                    && self.copiable_program(id).is_some_and(|program| program.flashback.is_some())),
            BodyTarget::AnyGraveyardCard => {
                matches!(target, TargetRef::Object(id) if self.window.cards().get(id).is_some_and(|object| object.card_id().is_some() && matches!(object.zone(), ObjectZone::Graveyard(_))))
            }
            BodyTarget::AnyGraveyardCreatureOrPlaneswalker => {
                matches!(target, TargetRef::Object(id)
                if self.window.cards().get(id).is_some_and(|card| card.card_id().is_some() && matches!(card.zone(), ObjectZone::Graveyard(_)))
                    && evaluated.get(id).is_some_and(|current| current.is_creature() || current.has_type(CardType::Planeswalker)))
            }
            BodyTarget::FilteredOwnGraveyardCard(filter) => {
                let TargetRef::Object(id) = target else {
                    return false;
                };
                if !self.window.cards().get(id).is_some_and(|o| {
                    o.zone() == ObjectZone::Graveyard(controller) && o.card_id().is_some()
                }) {
                    return false;
                }
                let Some(p) = evaluated.get(id) else {
                    return false;
                };
                self.matches_graveyard_card(filter, p)
            }
            BodyTarget::FilteredAnyGraveyardCard {
                filter,
                opponents_only,
            } => {
                let TargetRef::Object(id) = target else {
                    return false;
                };
                let Some(card) = self.window.cards().get(id) else {
                    return false;
                };
                let ObjectZone::Graveyard(owner) = card.zone() else {
                    return false;
                };
                if card.card_id().is_none() {
                    return false;
                }
                if opponents_only
                    && !self
                        .combat_rules
                        .opponents(self.window.participants(), controller)
                        .is_ok_and(|opponents| opponents.contains(&owner))
                {
                    return false;
                }
                let Some(p) = evaluated.get(id) else {
                    return false;
                };
                self.matches_graveyard_card(filter, p)
            }
            BodyTarget::SingleOwnGraveyardCard => {
                matches!(target,TargetRef::Object(id) if self.window.cards().get(id).is_some_and(|o|o.zone()==ObjectZone::Graveyard(controller) && o.card_id().is_some()))
            }
            BodyTarget::SingleCreature => matches!(target, TargetRef::Object(id)
                if self.window.cards().get(id).is_some_and(|card| card.zone() == ObjectZone::Battlefield)
                    && evaluated.get(id).is_some_and(ObjectCharacteristics::is_creature)),
            BodyTarget::SingleOpponent => {
                matches!(target, TargetRef::Player(seat) if self.combat_rules.opponents(self.window.participants(), controller).is_ok_and(|opponents| opponents.contains(&seat)))
            }
            BodyTarget::SinglePlayer => {
                matches!(target, TargetRef::Player(player) if self.window.participants().contains(&player))
            }
            BodyTarget::SinglePlayerOrPlaneswalker if matches!(target, TargetRef::Player(_)) => {
                matches!(target, TargetRef::Player(player) if self.window.participants().contains(&player))
            }
            BodyTarget::SingleOpponentOrPlaneswalker if matches!(target, TargetRef::Player(_)) => {
                matches!(target, TargetRef::Player(player) if self.combat_rules.opponents(self.window.participants(), controller).is_ok_and(|opponents| opponents.contains(&player)))
            }
            BodyTarget::OpponentOrPlaneswalkerThenCreature => match target {
                TargetRef::Player(player) => self
                    .combat_rules
                    .opponents(self.window.participants(), controller)
                    .is_ok_and(|opponents| opponents.contains(&player)),
                TargetRef::Object(id) => evaluated.get(id).is_some_and(|object| {
                    object.zone == ObjectZone::Battlefield
                        && object.has_type(CardType::Planeswalker)
                }),
                TargetRef::Stack(_) => false,
            },
            BodyTarget::SinglePlayerOrPlaneswalker
            | BodyTarget::SingleOpponentOrPlaneswalker
            | BodyTarget::SinglePlaneswalker => {
                matches!(target, TargetRef::Object(id)
                if self.window.cards().get(id).is_some_and(|card| card.zone() == ObjectZone::Battlefield)
                    && evaluated.get(id).is_some_and(|view| view.has_type(CardType::Planeswalker)))
            }
            BodyTarget::SingleAnyTarget => match target {
                TargetRef::Player(player) => self.window.participants().contains(&player),
                TargetRef::Object(id) => evaluated
                    .get(id)
                    .and_then(ObjectCharacteristics::damage_recipient)
                    .is_some(),
                TargetRef::Stack(_) => false,
            },
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
            BodyTarget::ControlledAbility { colorless_source } => {
                matches!(target,TargetRef::Stack(id) if self.window.entries().iter().any(|ability|
                ability.id()==id && ability.controller()==controller && !matches!(ability.kind(),StackKind::CardSpell(_) | StackKind::SpellCopy {..})
                    && (!colorless_source || self.targeting_entry_colors(ability,evaluated).is_empty())))
            }
            BodyTarget::StackEntryTargetingControlledPermanent => {
                self.stack_entry_targets_controlled_permanent(target, controller, evaluated, None)
            }
            BodyTarget::SingleStackEntry => {
                matches!(target, TargetRef::Stack(id) if self.window.entries().iter().chain(self.window.resolving()).any(|entry| entry.id() == id))
            }
            BodyTarget::SingleStackObject(filter) => match target {
                TargetRef::Stack(id) => self
                    .window
                    .entries()
                    .iter()
                    .chain(self.window.resolving())
                    .find(|entry| entry.id() == id)
                    .is_some_and(|entry| match entry.kind() {
                        StackKind::ActivatedAbility { .. } | StackKind::TriggeredAbility { .. } => {
                            let target_ok = match filter.target_shape {
                                crate::program::SpellTargetShape::Any => true,
                                crate::program::SpellTargetShape::SingleTarget => {
                                    entry.targets().targets().len() == 1
                                }
                                crate::program::SpellTargetShape::TargetingPlayer => entry
                                    .targets()
                                    .targets()
                                    .iter()
                                    .any(|t| matches!(t.choice().target, TargetRef::Player(_))),
                                crate::program::SpellTargetShape::TargetingOnlySource => false,
                            };
                            let controller_ok = match filter.object {
                                crate::program::ObjectFilter::Clause(c) => match c.controller {
                                    crate::program::ControllerFilter::Any => true,
                                    crate::program::ControllerFilter::You => {
                                        entry.controller() == controller
                                    }
                                    crate::program::ControllerFilter::Opponent
                                    | crate::program::ControllerFilter::NotYou => {
                                        entry.controller() != controller
                                    }
                                    crate::program::ControllerFilter::Player(seat) => {
                                        entry.controller() == seat
                                    }
                                    _ => true,
                                },
                                crate::program::ObjectFilter::Either(_, _) => true,
                            };
                            target_ok && controller_ok
                        }
                        StackKind::CardSpell(_) | StackKind::SpellCopy { .. } => self
                            .legal_target_kind_with_source(
                                BodyTarget::SingleSpell(filter),
                                target,
                                controller,
                                source,
                                evaluated,
                            ),
                        StackKind::Speed
                        | StackKind::Monarch
                        | StackKind::DelayedCopy { .. }
                        | StackKind::Emblem { .. } => false,
                    }),
                _ => false,
            },
            BodyTarget::SingleSpell(filter) => match target {
                TargetRef::Stack(id) => self
                    .window
                    .entries()
                    .iter()
                    .chain(self.window.resolving())
                    .find(|entry| entry.id() == id)
                    .is_some_and(|entry| {
                        let from_owners_hand = match entry.kind() {
                            StackKind::CardSpell(object) => self
                                .window
                                .cards()
                                .get(*object)
                                .is_some_and(|card| entry.cast_from_hand_of(card.owner())),
                            StackKind::SpellCopy { .. } => false,
                            _ => return false,
                        };
                        let zone_ok = match filter.cast_zone {
                            crate::program::SpellCastZoneFilter::Any => true,
                            crate::program::SpellCastZoneFilter::NotOwnersHand => !from_owners_hand,
                            crate::program::SpellCastZoneFilter::Exile => {
                                entry.cast_summary().cast_from_exile
                            }
                            crate::program::SpellCastZoneFilter::Hand => entry.cast_from_any_hand(),
                            crate::program::SpellCastZoneFilter::Graveyard => {
                                entry.cast_from_graveyard
                            }
                            crate::program::SpellCastZoneFilter::OwnLibrary => {
                                entry.cast_from_library == Some(controller)
                            }
                        };
                        if !zone_ok {
                            return false;
                        }
                        let target_ok = match filter.target_shape {
                            crate::program::SpellTargetShape::Any => true,
                            crate::program::SpellTargetShape::SingleTarget => {
                                entry.targets().targets().len() == 1
                            }
                            crate::program::SpellTargetShape::TargetingPlayer => {
                                entry.targets().targets().iter().any(|target| {
                                    matches!(target.choice().target, TargetRef::Player(_))
                                })
                            }
                            crate::program::SpellTargetShape::TargetingOnlySource => false,
                        };
                        if !target_ok {
                            return false;
                        }
                        if let Some(number) = filter.ordinal_this_turn
                            && self
                                .spell_cast_positions
                                .as_ref()
                                .and_then(|history| history.positions.get(&entry.id()))
                                != Some(&u64::from(number.get()))
                        {
                            return false;
                        }
                        if matches!(
                            filter.special,
                            crate::program::SpellSpecialFilter::CopyableInstruction
                        ) && !self.copyable_payload(id)
                        {
                            return false;
                        }
                        if filter.object == crate::program::ObjectFilter::any() {
                            return true;
                        }
                        let controller_ok = match filter.object {
                            crate::program::ObjectFilter::Clause(c) => match c.controller {
                                crate::program::ControllerFilter::Any => true,
                                crate::program::ControllerFilter::You => {
                                    entry.controller() == controller
                                }
                                crate::program::ControllerFilter::Opponent
                                | crate::program::ControllerFilter::NotYou => {
                                    entry.controller() != controller
                                }
                                crate::program::ControllerFilter::Player(seat) => {
                                    entry.controller() == seat
                                }
                                crate::program::ControllerFilter::ActivePlayer => {
                                    entry.controller() == self.window.active_player()
                                }
                                crate::program::ControllerFilter::OwnerYou
                                | crate::program::ControllerFilter::OwnerOpponent
                                | crate::program::ControllerFilter::RelatedPlayer
                                | crate::program::ControllerFilter::TargetPlayer
                                | crate::program::ControllerFilter::TargetOpponent
                                | crate::program::ControllerFilter::DefendingPlayer
                                | crate::program::ControllerFilter::FirstTargetController => false,
                            },
                            crate::program::ObjectFilter::Either(_, _) => true,
                        };
                        if !controller_ok {
                            return false;
                        }
                        match entry.kind() {
                            StackKind::CardSpell(object) => {
                                evaluated.get(*object).is_some_and(|card| {
                                    filter.object.matches_static_characteristics(
                                        card,
                                        &BTreeSet::new(),
                                        || Some(card.defined.mana_value.clone()),
                                    )
                                })
                            }
                            StackKind::SpellCopy { .. } => self
                                .stack_copiable_card_types(id)
                                .is_some_and(|types| filter.matches_card_types(types)),
                            _ => false,
                        }
                    }),
                _ => false,
            },
        }
    }
    /// Enumerate once against the same current view and predicate as normal targets.
    fn trigger_target_domain(
        &self,
        body: &ExecutableBody,
        controller: SeatId,
        source: Option<&crate::occurrence::SupportedAppearance>,
    ) -> Result<Vec<TargetRef>, EffectError> {
        let evaluated = self.evaluated_characteristics()?;
        let protection_source = source
            .map(|s| self.waiting_protection_source(s, &evaluated))
            .unwrap_or_default();
        let source_ctx = TargetSourceContext {
            id: source.map(|s| s.object.id()),
            names: source.map(|s| &s.current_names),
        };
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
            .filter(|target| {
                self.mentor_target(body, *target, source, &evaluated)
                    && !Self::protected_target(*target, &protection_source, &evaluated)
                    && self.legal_body_target_in_with_source(
                        body, *target, controller, source_ctx, &evaluated,
                    )
            })
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
            Instruction::AffectPermanentTargets { .. }
            | Instruction::MillEachPlayer(_)
            | Instruction::PutLibraryCohortIntoGraveyard(_)
            | Instruction::ExileOnePerPlayerUntilSourceLeaves
            | Instruction::AttachControlledToSource { .. }
            | Instruction::AttachBattlefieldAurasToSource
            | Instruction::EnterHandOrGraveyardAurasAttachedToSource
            | Instruction::ReturnDelayedGraveyardToHand(_)
            | Instruction::LoseLifeAndScryForSubtype(_)
            | Instruction::GiveEquippedCreatureToUpkeepPlayer
            | Instruction::CopyCreatureExiledForAbility
            | Instruction::DamageFromGrantingPermanent { .. }
            | Instruction::CopyEntrantAsArtifact
            | Instruction::ReplaceSourceCreatedTokens
            | Instruction::CopyEntrantOnceEachTurn
            | Instruction::CopyCreatureAsToken
            | Instruction::CopyDifferentlyNamedTokens
            | Instruction::CopyOtherControlledNontokensAsEntryHost
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
            | Instruction::Discard { .. }
            | Instruction::DiscardFiltered { .. } => {
                return Err(EffectError::InconsistentProgram);
            }
            Instruction::AdjustTargetLife { .. }
            | Instruction::DivideTargetPermanentsAndSacrificePile => match target {
                TargetRef::Player(player) => self.window.participants().contains(&player),
                TargetRef::Object(_) | TargetRef::Stack(_) => false,
            },
            Instruction::DamageTargetControllersCreatures { .. } => match target {
                TargetRef::Player(player) => self.window.participants().contains(&player),
                TargetRef::Object(id) => self
                    .window
                    .cards()
                    .get(id)
                    .is_some_and(|o| o.zone() == ObjectZone::Battlefield),
                TargetRef::Stack(_) => false,
            },
            Instruction::CounterSpell
            | Instruction::CounterSpellToExile
            | Instruction::CounterSpellToLibrary { .. } => {
                matches!(target,TargetRef::Stack(id) if self.window.entries().iter().any(|entry|entry.id()==id))
            }
            Instruction::ChooseNewTargets
            | Instruction::GainControlOfSpell
            | Instruction::ChangeOnlyTarget
            | Instruction::LoseLifeEqualToTargetSpellManaValue => {
                matches!(target, TargetRef::Stack(id) if self.window.entries().iter().any(|entry| entry.id() == id))
            }
            Instruction::CopyAbility => {
                matches!(target,TargetRef::Stack(id) if self.window.entries().iter().any(|s|s.id()==id && !matches!(s.kind(),StackKind::CardSpell(_) | StackKind::SpellCopy {..})))
            }
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
            Instruction::DamageFromPower(_)
            | Instruction::DamageAmount(_)
            | Instruction::DamageAmountThenExileIfDies(_)
            | Instruction::DamageCreatureAtSpellMastery { .. }
            | Instruction::Damage { .. } => match target {
                TargetRef::Player(player) => self.window.participants().contains(&player),
                TargetRef::Object(card) => evaluated
                    .get(card)
                    .and_then(ObjectCharacteristics::damage_recipient)
                    .is_some(),
                TargetRef::Stack(_) => false,
            },
            Instruction::Attach(_) => true,
            Instruction::RegenerateTarget => {
                matches!(target, TargetRef::Object(id) if evaluated.get(id).is_some_and(|p| p.zone == ObjectZone::Battlefield))
            }
            Instruction::RemoveTargetFromCombat => matches!(target, TargetRef::Object(id)
                if self.combat.as_ref().is_some_and(|combat| combat.attacker(id).is_some() || combat.blocker(id).is_some())),
            Instruction::GoadTarget | Instruction::RequireTargetAttack => {
                matches!(target, TargetRef::Object(id) if evaluated.get(id).is_some_and(|p| p.zone == ObjectZone::Battlefield && p.is_creature()))
            }
            Instruction::DestroyTarget | Instruction::DestroyTargetWithoutRegeneration => {
                matches!(target, TargetRef::Object(id) if evaluated.get(id).is_some_and(|view| view.zone == ObjectZone::Battlefield && !view.has_keyword(KeywordAbility::Indestructible)))
            }
            Instruction::ExileCreatureAtMostRevealedPower
            | Instruction::ExileTarget
            | Instruction::ExileTargetWithLink(_)
            | Instruction::ImprintTargetLand
            | Instruction::ImprintTargetCreature
            | Instruction::ExileTargetAsAbilityDonor
            | Instruction::ExileTargetUntilSourceLeaves
            | Instruction::ExileUntilNextEndStep
            | Instruction::ExileTargetUntilOpponentMonarch => {
                matches!(target, TargetRef::Object(id) if self.window.cards().get(id).is_some_and(|o| o.zone() == ObjectZone::Battlefield))
            }
            Instruction::ReturnToOwnersHand | Instruction::PutTargetInLibrary(_) => {
                matches!(target, TargetRef::Object(id)
                if self.window.cards().get(id).is_some_and(|o| matches!(o.zone(), ObjectZone::Battlefield | ObjectZone::Graveyard(_) | ObjectZone::Exile)))
            }
            Instruction::ExileForBlackToken { .. }
            | Instruction::ExileGraveyardTarget
            | Instruction::ReturnTargetGraveyardCreatureToHand => {
                matches!(target, TargetRef::Object(id) if evaluated.get(id).is_some_and(|card| matches!(card.zone, ObjectZone::Graveyard(_))))
            }
            Instruction::ReturnAnyGraveyardCreatureBlackSubtype(_)
            | Instruction::ReturnOwnGraveyardCreatureToBattlefield => {
                matches!(target, TargetRef::Object(id)
                if self.window.cards().get(id).is_some_and(|o| matches!(o.zone(), ObjectZone::Graveyard(_)))
                    && evaluated.get(id).is_some_and(ObjectCharacteristics::is_creature))
            }
            Instruction::ReturnOwnGraveyardPermanentToBattlefield { .. }
            | Instruction::ReturnAnyGraveyardCreatureOrPlaneswalker => {
                matches!(target, TargetRef::Object(id)
                if self.window.cards().get(id).is_some_and(|o| matches!(o.zone(), ObjectZone::Graveyard(_)))
                    && evaluated.get(id).is_some_and(|value| value.current_types.iter().copied().any(CardType::is_permanent)))
            }
            Instruction::CreatePreventionShield { .. }
            | Instruction::CreateReflectingShield
            | Instruction::CreateDamageMultiplier { .. }
            | Instruction::CreateContinuousEffect { .. }
            | Instruction::WeakenTargetByChosenX
            | Instruction::BoostTargetPower { .. }
            | Instruction::AnimateTargetAsCountedCreature { .. }
            | Instruction::CopyTargetToSource { .. }
            | Instruction::GrantProtectionChosenColor(_)
            | Instruction::PutIndestructibleCounter
            | Instruction::PutLoyaltyCounter
            | Instruction::PutNamedCounterOnTarget { .. }
            | Instruction::PutPowerToughnessCounters { .. }
            | Instruction::PhaseOut(crate::program::PhasingSubject::Target)
            | Instruction::DoublePowerToughness {
                target_self: false, ..
            }
            | Instruction::CreatureDealsPowerDamageToOther { .. }
            | Instruction::WatchTargetCombatDamageDraw => match target {
                TargetRef::Object(card) => self
                    .window
                    .cards()
                    .get(card)
                    .is_some_and(|value| value.zone() == ObjectZone::Battlefield),
                TargetRef::Player(seat) => {
                    matches!(
                        instruction,
                        Instruction::CreatePreventionShield { .. }
                            | Instruction::CreateReflectingShield
                    ) && self.window.participants().contains(&seat)
                }
                TargetRef::Stack(_) => false,
            },
            // The iterative driver owns these paths before target availability.
            Instruction::OptionalSequence { .. }
            | Instruction::OptionalEffect { .. }
            | Instruction::ExileForSubtypeToken(_)
            | Instruction::OfferManaPayment { .. }
            | Instruction::BranchIfFalse { .. }
            | Instruction::Jump { .. }
            | Instruction::BoostSaddledAttacker { .. }
            | Instruction::SetControlledCreatureSizeToHand
            | Instruction::ModifyGroupByAmount { .. }
            | Instruction::AddExperienceCounter
            | Instruction::AddPlayerCounter { .. }
            | Instruction::ExileTopPlay { .. }
            | Instruction::LookTopChoose { .. }
            | Instruction::MoveSubject { .. }
            | Instruction::SelectZoneCardsToZone { .. }
            | Instruction::ChooseOption { .. }
            | Instruction::DealDamageFrom { .. }
            | Instruction::DivideDamageAmongTargets { .. }
            | Instruction::GainControlOfPermanent { .. }
            | Instruction::GiveFixedTargetControl { .. }
            | Instruction::BecomeCopy { .. }
            | Instruction::BecomeCopyFixedTargets { .. }
            | Instruction::DoublePowerToughnessSubject { .. }
            | Instruction::DoubleCountersOnSubject { .. }
            | Instruction::ShuffleSubjectIntoLibrary { .. }
            | Instruction::WinGame(_)
            | Instruction::LoseGame(_)
            | Instruction::DistributeCountersAmongTargets { .. }
            | Instruction::RollDie { .. }
            | Instruction::FlipCoins { .. }
            | Instruction::ChooseValue { .. }
            | Instruction::ChooseOptionForPlayers { .. }
            | Instruction::Vote { .. }
            | Instruction::Incubate { .. }
            | Instruction::Connive { .. }
            | Instruction::ConniveFinalize { .. }
            | Instruction::Discover { .. }
            | Instruction::Clash { .. }
            | Instruction::RingTemptsYou
            | Instruction::VentureIntoDungeon { .. }
            | Instruction::TakeInitiative { .. }
            | Instruction::UnlockDoor { .. }
            | Instruction::Behold { .. }
            | Instruction::ManifestDread
            | Instruction::Monstrosity { .. }
            | Instruction::Fight { .. }
            | Instruction::Exchange(_)
            | Instruction::CopyStackObject { .. }
            | Instruction::ExileGroupFromZone { .. }
            | Instruction::SacrificeSubject { .. }
            | Instruction::DestroyChosen { .. }
            | Instruction::CastLinkedFaceDownFree
            | Instruction::Ripple(_)
            | Instruction::Demonstrate
            | Instruction::Enlist
            | Instruction::AttachPermanent { .. }
            | Instruction::SkipNextUntap { .. }
            | Instruction::RequireCombatAction { .. }
            | Instruction::CreateTemporaryRule { .. }
            | Instruction::DestroySubject { .. }
            | Instruction::GrantCardPlayPermission { .. }
            | Instruction::CreateTokenCopy { .. }
            | Instruction::BindResolutionX(_)
            | Instruction::PutResolvingSpellInLibrary(_)
            | Instruction::PutSubjectIntoLibraryFromTop { .. }
            | Instruction::SetDayNight(_)
            | Instruction::CreateAuraTokenAttached { .. }
            | Instruction::ExploreSubject(_)
            | Instruction::ScheduleDelayedEventTrigger { .. }
            | Instruction::MoveGraveyardGroup { .. }
            | Instruction::PutHandPermanentFiltered { .. }
            | Instruction::RepeatLoop { .. }
            | Instruction::RevealUntilMatch { .. }
            | Instruction::ControlManaSourceThroughNextTurn
            | Instruction::GrantControllerFlashUntilCleanup
            | Instruction::RequireOpponentsCreaturesBlockThisTurn
            | Instruction::RequireBlocksAgainstTargetThisTurn
            | Instruction::SuppressGroupKeywords { .. }
            | Instruction::ModifyGroupUntilEndOfTurn { .. }
            | Instruction::BoostSource { .. }
            | Instruction::ResolvePowerAbility(_)
            | Instruction::Adapt(_)
            | Instruction::PutCountersOnSource { .. }
            | Instruction::UnattachEquipmentAndProtectHost
            | Instruction::Unattach
            | Instruction::CreateTokenAndAttach { .. }
            | Instruction::CreateTokensToFight(_)
            | Instruction::FightPairedCreatures(_)
            | Instruction::OfferOpponentsDamage
            | Instruction::TemptWithTokens { .. }
            | Instruction::CreateTokens { .. }
            | Instruction::CreateAttackingTokens { .. }
            | Instruction::Scry { .. }
            | Instruction::Surveil { .. }
            | Instruction::Mill { .. }
            | Instruction::OfferSourceCounters { .. }
            | Instruction::IncreaseSpeed
            | Instruction::BecomeMonarch(_)
            | Instruction::TapEnchantedCreature
            | Instruction::RequireCondition(_)
            | Instruction::PayControllerLife(_)
            | Instruction::PutEquipmentFromHandOrGraveyard
            | Instruction::EquipNewToken
            | Instruction::Encore
            | Instruction::GrantNewTokensHaste
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
            | Instruction::ReturnCardsDestroyedThisWay
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
            | Instruction::ModifyAnyNumberOfPermanents { .. }
            | Instruction::ScheduleCreatedTokenExile(_)
            | Instruction::ScheduleCreatedTokenSacrifice
            | Instruction::ExileDelayedObjects(_)
            | Instruction::ExileRelatedPermanents(_)
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
            | Instruction::PhaseOtherPlaneswalkersNextEndStep
            | Instruction::ChooseMeldPartner(_)
            | Instruction::ExileMeldPair
            | Instruction::MeldExiledPair(_)
            | Instruction::PairWithCreature { .. }
            | Instruction::PhaseOut(
                crate::program::PhasingSubject::Source
                | crate::program::PhasingSubject::ControlledLands,
            )
            | Instruction::PhaseOutObjects(_)
            | Instruction::ReturnOneNonlandPerOpponent
            | Instruction::WeakenDelayedAttacker(_)
            | Instruction::DrawRevealRemoveLoyalty
            | Instruction::CreateReflexiveDiscardDamage
            | Instruction::ScheduleDelayedStepTrigger { .. }
            | Instruction::EmitReflexiveTrigger { .. }
            | Instruction::RemoveLoyaltyExileTopForPlay
            | Instruction::DamageDamagedPlayersCreatures
            | Instruction::DamageOpponentsAmount(_)
            | Instruction::DamageRelatedPlayerAmount(_)
            | Instruction::DamageGroupAmount { .. }
            | Instruction::DoublePowerToughness {
                target_self: true, ..
            }
            | Instruction::Ninjutsu
            | Instruction::Unearth
            | Instruction::FlipCoin { .. }
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
            | Instruction::ReturnDelayedPermanentToHand(_)
            | Instruction::ReturnDelayedExileWithCounters(_)
            | Instruction::ReturnDelayedExile(_)
            | Instruction::ExileUnearthed(_)
            | Instruction::ResolveTransfer(_)
            | Instruction::DrainOpponents(_)
            | Instruction::ExtortDrain
            | Instruction::ResolveGrowth(_)
            | Instruction::ResolveCounterKeyword(_)
            | Instruction::SacrificeEvoked
            | Instruction::SacrificeStateTriggerSource
            | Instruction::SacrificeUpkeepSource(_)
            | Instruction::CounterWardSource
            | Instruction::CounterTriggeringStackObject
            | Instruction::ResolveKeywordTrigger(_)
            | Instruction::BoostSourceByCastManaValue
            | Instruction::BoostLoneAttacker
            | Instruction::DrawTargetPlayer { .. }
            | Instruction::DrawPlayer { .. }
            | Instruction::DrawHighestEntrantController
            | Instruction::DrawRelatedPlayer
            | Instruction::DrawCombatDamageSourceController
            | Instruction::DrawAndLoseLifeForTargetGraveyardCreatures
            | Instruction::DrawControllerAndTarget { .. }
            | Instruction::DrawEachPlayer { .. }
            | Instruction::DrawPlayersControllingName(_)
            | Instruction::DrawController { .. }
            | Instruction::ReturnChosenXGraveyardCreatures
            | Instruction::ReturnGraveyardTargetsTapped(_)
            | Instruction::ReturnCapturedDyingCreature
            | Instruction::ReturnCapturedDyingCreatureWithTypeAndColor { .. }
            | Instruction::ReturnGraveyardCreatureWithSubtypeCounters { .. }
            | Instruction::ReturnGraveyardCohort(_)
            | Instruction::DivideThreeDamageAmongTargets
            | Instruction::DistributePlusOneCounters { .. }
            | Instruction::DamageOrderedTargets { .. }
            | Instruction::ExileUntilOtherTargetLeaves
            | Instruction::CopyOtherTargetAsArtifact
            | Instruction::FixedTargetClause(_)
            | Instruction::ExileResolvingSpell
            | Instruction::ReturnResolvingSpellToHand
            | Instruction::Ascend
            | Instruction::DestroyFixedTargets
            | Instruction::DestroyTargetAndAttachedEquipment
            | Instruction::ReturnFixedTargets
            | Instruction::FightFixedTargets
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
            | Instruction::RevealSharedTypeCreatureCastOrBottom
            | Instruction::RevealOddCastOrDraw
            | Instruction::MayTurnControlledCreatureFaceUp
            | Instruction::ExploreSource
            | Instruction::MayRevealLibraryTop
            | Instruction::ManifestTop { .. }
            | Instruction::ExileNonlandsToHandAndDamage { .. }
            | Instruction::RevealTopHandLoseManaValue { .. }
            | Instruction::RevealUntilLandKind
            | Instruction::RevealOpponentChoosesCreature
            | Instruction::ShuffleAbilitySourceIntoLibrary
            | Instruction::RevealToNameAfterSourceShuffle(_)
            | Instruction::WatchSmallCreatureCombatDraw(_)
            | Instruction::ChooseOwnCommanderToCast
            | Instruction::ChooseMatchingHandSpellToCast
            | Instruction::SearchEachLibraryForFreeCast
            | Instruction::ChooseFoundSpell
            | Instruction::CastFoundSpell(_)
            | Instruction::LookTopOneRestGraveyard { .. }
            | Instruction::RevealTopHandRestGraveyard { .. }
            | Instruction::LookTopOneRestBottom { .. }
            | Instruction::LookTopCreatureSharingAttachedSubtype { .. }
            | Instruction::RevealTopCreaturesAndLandsRestBottom { .. }
            | Instruction::RevealHandAndBottomPiles
            | Instruction::RevealDividePiles
            | Instruction::DrawToChosenOpponentHandSize
            | Instruction::LookTopPlaneswalkers
            | Instruction::RevealTopPermanentOrHand
            | Instruction::LookTopLandBattlefieldTapped
            | Instruction::LookTopReveal { .. }
            | Instruction::LookTopTakeOne { .. }
            | Instruction::SearchLibrary { .. }
            | Instruction::ChooseTypeThenDestroyUnchosen
            | Instruction::OrderArtifactCreatureLandSacrifices
            | Instruction::ChooseCreatureTypeForPlayer { .. }
            | Instruction::ChooseCreatureType { .. }
            | Instruction::RepeatOpponentLifeLoss
            | Instruction::OpponentsLoseAndControllerGains(_)
            | Instruction::AdjustEachOpponentLife { .. }
            | Instruction::AdjustPlayersLife { .. }
            | Instruction::RememberPermanentTarget
            | Instruction::ControllerLosesRememberedManaValue
            | Instruction::ExiledTargetControllerGainsPower
            | Instruction::GoadOtherPlayersCreatures
            | Instruction::AdditionalCombat { .. }
            | Instruction::AdditionalCombatAfterUntappingAttackers
            | Instruction::TakeExtraTurn
            | Instruction::DisallowDamagePreventionThisTurn
            | Instruction::GrantNoMaxHandSizeRestOfGame
            | Instruction::PreventDamageThisTurn(_)
            | Instruction::MoveGroup { .. }
            | Instruction::PutOwnHandCardOnLibraryBottom
            | Instruction::PutGroupOnLibraryBottom(_)
            | Instruction::ExileCapturedGraveyardCard
            | Instruction::GrantGraveyardCastAndWatchAbilities
            | Instruction::GainCastCardAbilities { .. }
            | Instruction::ExileCardsFromGraveyard { .. }
            | Instruction::ReturnUpToOneRecentGraveyardCard
            | Instruction::ExileUpToTwoGraveyardTargetsGainLife
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
            | Instruction::ShufflePlayerLibrary(_)
            | Instruction::ShuffleTargetIntoLibrary
            | Instruction::RevealPlayerHand(_)
            | Instruction::ShuffleOwnGraveyardIntoLibrary
            | Instruction::ShuffleTargetGraveyardCards { .. }
            | Instruction::ExileCreaturesAndPlaneswalkersBelowRevealedManaValue
            | Instruction::CastLinkedSpellMatchingX
            | Instruction::DestroyDamagedPlayersNonlandsAtX
            | Instruction::PutChosenXChargeCounters
            | Instruction::DestroyNonlandsAtSourceChargeCount
            | Instruction::AddCounterAndDischargeOnResolution(_)
            | Instruction::ExileAllGraveyards
            | Instruction::ExilePlayerGraveyards(_)
            | Instruction::PayControllerEnergy(_)
            | Instruction::RemoveCountersFromPermanent { .. }
            | Instruction::ExileDefeatedSiege
            | Instruction::ExileSourceAndGainLife(_)
            | Instruction::ExileAbilitySource
            | Instruction::ReturnSourceToHand
            | Instruction::PutCounterOnRelatedPermanent
            | Instruction::PutCountersOnControlledCreatures
            | Instruction::PutCountersOnControlledSubtype(_)
            | Instruction::PutCountersOnControlledCreaturesAndGrant(_)
            | Instruction::PutCountersOnOtherCreatures
            | Instruction::PutCreatureAndOtherPlaneswalkerCounters
            | Instruction::PutChargeCounterOnSource
            | Instruction::PutBlessingCounterOnSource
            | Instruction::PutNamedCounterOnSource { .. }
            | Instruction::PutCountersOnSubject { .. }
            | Instruction::SetGroupTapped { .. }
            | Instruction::SetSubjectTapped { .. }
            | Instruction::PutQuestCounterOnSource
            | Instruction::PutStorageCounterOnSource
            | Instruction::AbilitySourceControllerLosesLife(_)
            | Instruction::RelatedPlayerLosesLife(_)
            | Instruction::LinkedPlayerLosesDamageAmount
            | Instruction::RelatedPlayerLosesHalfLife
            | Instruction::FightSourceWithTarget
            | Instruction::ClearSourceCounters(_)
            | Instruction::TransformSource
            | Instruction::TransformSourceAndAttachToTarget
            | Instruction::AdvanceClassLevel(_)
            | Instruction::SolveSourceCase
            | Instruction::BecomePreparedSource
            | Instruction::TransformSubtype(_)
            | Instruction::UntapAbilitySource
            | Instruction::TapAbilitySource
            | Instruction::RegenerateSource
            | Instruction::RegenerateControlledSubtype(_)
            | Instruction::DestroyGroupWithoutRegeneration(_)
            | Instruction::DestroyGroup(_)
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
            | Instruction::ChangeGroup { .. }
            | Instruction::ChangeAbilitySource { .. }
            | Instruction::GiveControlOfAbilitySource
            | Instruction::UntapAndControlCreatures
            | Instruction::Proliferate
            | Instruction::DamagePlayersHalfLife
            | Instruction::DamageGroup { .. }
            | Instruction::DrawAndDamageOpponents { .. }
            | Instruction::GainControllerLife { .. }
            | Instruction::DrainTargetAndGain(_)
            | Instruction::AdjustControllerLife { .. }
            | Instruction::PayLife { .. }
            | Instruction::PayEnergy { .. }
            | Instruction::ExileFromZone { .. }
            | Instruction::TapOrUntapControlled { .. }
            | Instruction::ExertAbilitySource
            | Instruction::CollectEvidence { .. }
            | Instruction::Blight { .. }
            | Instruction::OfferDynamicManaPayment { .. }
            | Instruction::ExchangeLifeWithSourceToughness
            | Instruction::ForEachPlayer { .. }
            | Instruction::OfferAnyPlayer { .. }
            | Instruction::DealDamageToAnnouncedTargets { .. }
            | Instruction::GrantChosenProtection { .. }
            | Instruction::RemoveCountersFromSubject { .. }
            | Instruction::MoveGroupToLibrary { .. }
            | Instruction::ExileLibraryTopLinked { .. }
            | Instruction::UntapOrTapSelect { .. }
            | Instruction::MoveGraveyardGroupOwned { .. }
            | Instruction::LookTopChooseExt { .. }
            | Instruction::RevealUntilMatchExt { .. }
            | Instruction::CreateTokenCopyExt { .. }
            | Instruction::MoveCounters { .. }
            | Instruction::ExileSubjectUntilSourceLeaves { .. }
            | Instruction::ReturnCardsExiledWithLinkExt { .. }
            | Instruction::DrawGame
            | Instruction::TurnFaceUpSubject { .. } => false,
        })
    }

    fn prune_payloads(&mut self) {
        let live: BTreeSet<_> = self
            .window
            .entries()
            .iter()
            .chain(self.window.resolving())
            .map(|entry| entry.id())
            .collect();
        self.payloads.retain(|id, _| live.contains(id));
        self.uncounterable_by_colors
            .retain(|id, _| live.contains(id));
        self.update_growth_last_known(&[]);
        self.prune_aura_graveyard_returns();
    }
    fn prune_non_battlefield_state(&mut self) {
        let current: BTreeSet<_> = self
            .window
            .cards()
            .physical_objects()
            .filter(|card| card.zone() == ObjectZone::Battlefield)
            .map(|card| card.id())
            .collect();
        self.encore_attack_targets
            .retain(|object, _| current.contains(object));
        self.damage.retain(|object, _| current.contains(object));
        self.deathtouch_damage
            .retain(|object| current.contains(object));
        self.characteristics.time_counters.retain(|id, _| {
            self.window.cards().phased_out_under(*id).is_some()
                || self.window.cards().get(*id).is_some_and(|o| {
                    matches!(o.zone(), ObjectZone::Battlefield | ObjectZone::Exile)
                })
        });
        self.entry_color_choices
            .retain(|object, _| current.contains(object));
        // An ability can still refer to the entry choice of its departed source.
        // Retain that incarnation only while a waiting or stacked ability can use it.
        let waiting = self
            .triggers
            .waiting()
            .values()
            .map(|ability| ability.source.object.id());
        let stacked =
            self.window
                .entries()
                .iter()
                .filter_map(|entry| match entry.kind() {
                    StackKind::ActivatedAbility { source }
                    | StackKind::TriggeredAbility { source } => Some(source.id()),
                    _ => None,
                });
        let referenced: BTreeSet<_> = waiting.chain(stacked).collect();
        let mut linked_counter_sources = BTreeSet::new();
        if !self.characteristics.named_counters.is_empty() {
            linked_counter_sources.extend(referenced.iter().copied());
            for card in self.window.cards().physical_objects() {
                if card.zone() == ObjectZone::Exile {
                    linked_counter_sources.insert(card.id());
                }
                let Some(Program {
                    continuous:
                        Some(crate::characteristics::StaticContinuousAbility::CharacteristicDefining {
                            parts,
                            ..
                        }),
                    ..
                }) = self.copiable_program(card.id())
                else {
                    continue;
                };
                for part in &parts.0 {
                    if let crate::characteristics::DefiningPart::PowerToughness {
                        power,
                        toughness,
                    } = part
                    {
                        for expression in
                            [power, toughness].into_iter().filter_map(Option::as_deref)
                        {
                            if let crate::characteristics::NumericExpression::Counters {
                                source: Some(source),
                                ..
                            } = expression
                            {
                                linked_counter_sources.insert(*source);
                            }
                        }
                    }
                }
            }
        }
        self.characteristics
            .retain_battlefield(&current, &linked_counter_sources);
        self.entry_creature_type_choices
            .retain(|object, _| current.contains(object) || referenced.contains(object));
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
                        BodyTarget::SingleSpell(SpellFilter::copyable_instruction()),
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

#[derive(Clone, Copy, Default)]
pub(super) struct TargetSourceContext<'a> {
    pub(super) id: Option<ObjectId>,
    pub(super) names: Option<&'a spellwake_contracts::names::ObjectNames>,
}

const fn same_name_ability_target() -> BodyTarget {
    BodyTarget::FilteredPermanent(
        crate::program::ObjectFilter::creature().sharing_name_with_source(),
    )
}
