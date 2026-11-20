//! Native game-scoped input boundary over the restricted executor.
//!
//! Candidates are owned values, not durable commits. This module supplies no auth,
//! clocks, wire/checkpoint codec, seat projection, replay or general Magic engine.
#[cfg(test)]
mod cast_reference_tests;
#[cfg(test)]
pub(crate) mod private_qualification;
#[cfg(test)]
mod related_player_fault_qualification;
#[cfg(test)]
mod token_fault_tests;
use crate::checks::{CheckBoundary, CheckDecision, CheckJournal};
#[cfg(test)]
use crate::combat::CombatRules;
use crate::combat::{CombatAssignmentRecord, CombatDecision, CombatDeclarationRecord};
use crate::effects::{ManaActivationResult, RuleResult};
#[cfg(test)]
use crate::mana::ManaAbilityId;
use crate::occurrence::{
    DrawOrigin, OpeningMovePhase, OpeningShufflePhase, RulesJournal, RulesTransition, TriggerOrder,
    TriggerTargetChoice,
};
#[cfg(test)]
use crate::program::{BodyTarget, ExecutableBody, SpellTargetFilter};
use crate::setup::GameSeed;
#[cfg(test)]
mod action_tests;
#[cfg(test)]
mod characteristics_remaining_tests;
#[cfg(test)]
mod characteristics_tests;
#[cfg(test)]
mod cleanup_tests;
mod conclusion;
mod facts;
pub use conclusion::{BankExpiryInput, GameConclusion, GameConclusionCause};
#[cfg(test)]
mod concession_tests;
#[cfg(test)]
mod lifecycle_tests;
mod opening_flow;
use facts::TransitionFacts;
#[cfg(test)]
mod legend_tests;
#[cfg(test)]
mod permanent_tests;
#[cfg(test)]
mod setup_tests;
#[cfg(test)]
mod shared_damage_tests;
#[cfg(test)]
mod world_history_tests;
use crate::actions::{LandPlayRecord, PermanentEntryRecord, TimingError};
use crate::activated::{AbilityActivationRecord, ActivatedAbilityId};
use crate::{
    damage::{DamageJournal, DamageOrigin},
    draw::{DrawJournal, GameOutcome},
    effects::{
        CopyResolution, DamageDecision, EffectDecision, EffectError, EffectWindow,
        ManaActivationProgress, ManaColorChoice, RuleCompletion, RuleProgress,
    },
    mana::{ManaPayment, ManaPaymentRecord, ManaProductionRecord},
    object_identity::{ObjectId, ObjectZone},
    priority::{PriorityPhase, SeatId},
    stack_window::WindowPhase,
    targets::TargetRef,
    turn::{
        PriorityCompleted, Step, TurnError, TurnPosition, TurnScheduler, TurnStage, TurnStart,
        TurnWork, WorkCompleted,
    },
};
#[cfg(test)]
use crate::{effects::SeedCard, mana::FloatingMana};
use crate::{
    opening::{
        HAND_SIZE, MulliganChoice, OpeningDecision, OpeningError, OpeningStage, OpeningState,
    },
    shuffle::{EntropyError, EntropyTape, ShuffleRecord},
};
use std::collections::{BTreeMap, BTreeSet};

/// Host-supplied identity; numeric object/seat identities remain local to this game.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GameId(pub u128);

/// Owned immutable source identities. Constructors validate shape, not source truth
/// or all-card qualification; the enclosing publisher must establish those claims.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GameContext {
    release: String,
    rules: String,
    catalogue: String,
    programs: String,
    profile: String,
    creature_vocabulary: spellwake_contracts::creature_types::CreatureVocabularyId,
    name_rules: spellwake_contracts::names::NameRulesId,
}
impl GameContext {
    pub fn new(
        release: String,
        rules: String,
        catalogue: String,
        programs: String,
        profile: String,
        creature_vocabulary: spellwake_contracts::creature_types::CreatureVocabularyId,
        name_rules: spellwake_contracts::names::NameRulesId,
    ) -> Result<Self, GameError> {
        if [&release, &rules, &catalogue, &programs, &profile]
            .iter()
            .any(|value| value.trim().is_empty() || value.trim() != value.as_str())
        {
            return Err(GameError::InvalidContext);
        }
        Ok(Self {
            release,
            rules,
            catalogue,
            programs,
            profile,
            creature_vocabulary,
            name_rules,
        })
    }
    pub fn release(&self) -> &str {
        &self.release
    }
    pub fn rules(&self) -> &str {
        &self.rules
    }
    pub fn catalogue(&self) -> &str {
        &self.catalogue
    }
    pub fn programs(&self) -> &str {
        &self.programs
    }
    pub fn profile(&self) -> &str {
        &self.profile
    }
    pub fn creature_vocabulary_id(
        &self,
    ) -> spellwake_contracts::creature_types::CreatureVocabularyId {
        self.creature_vocabulary
    }
    pub fn name_rules_id(&self) -> spellwake_contracts::names::NameRulesId {
        self.name_rules
    }
}

/// Exact expected input basis. External accepted-input revision is distinct from
/// the private executor's counters, which can advance several times per input.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GameBasis {
    pub game: GameId,
    pub context: GameContext,
    pub epoch: u64,
    pub revision: u64,
}

/// Decision identity local to its complete game/context/epoch/revision basis.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DecisionId(pub u64);

/// One canonical native frame/reply vocabulary shared with the executor.
pub use crate::effects::DecisionReply;
/// Game-level choice vocabulary; mana abilities have no stack-resolution frame.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuleFrame {
    ReplacementChooser(crate::effects::ReplacementChooserFrame),
    /// Privileged native precommit replacement request.
    ZoneReplacement(crate::effects::ZoneReplacementFrame),
    /// Privileged genuine library partition; not a production seat-safe view.
    LibraryTop(crate::effects::LibraryTopFrame),
    /// The executor's exact affected-player selection, under this root's authority.
    ResolutionSelect(crate::effects::ResolutionSelectionFrame),
    /// The executor's complete physical owner order, after all selections.
    ResolutionGraveyardOrder(crate::effects::ResolutionGraveyardOrderFrame),
    OptionalEffect(crate::effects::OptionalEffectFrame),
    ResolutionPayment(crate::effects::ResolutionPaymentFrame),
    TimestampOrder(crate::token::TokenTimestampOrder),
    Checks(CheckDecision),
    Triggers(TriggerOrder),
    TriggerTarget(TriggerTargetChoice),
    Opening(OpeningDecision),
    CopyTargets(CopyResolution),
    Damage(DamageDecision),
    ManaColor(ManaColorChoice),
    Combat(CombatDecision),
    CreatureType(crate::effects::CreatureTypeChoice),
}
impl RuleFrame {
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
            Self::Opening(f) => f.chooser(),
            Self::CopyTargets(f) => f.chooser(),
            Self::Damage(f) => f.chooser(),
            Self::ManaColor(f) => f.chooser(),
            Self::Combat(f) => f.chooser(),
            Self::CreatureType(f) => f.chooser(),
        }
    }
    /// Whether a reissued decision still asks for the same native response.
    /// Used to carry a running deadline across unrelated transitions. The
    /// command path separately authenticates the original, revision-bound frame.
    pub fn same_response_obligation(&self, other: &Self) -> bool {
        fn issuance_revision(frame: &mut RuleFrame) -> Option<&mut u64> {
            Some(match frame {
                RuleFrame::Opening(_) => return None,
                RuleFrame::ReplacementChooser(f) => &mut f.revision,
                RuleFrame::ZoneReplacement(f) => &mut f.revision,
                RuleFrame::LibraryTop(f) => &mut f.revision,
                RuleFrame::ResolutionSelect(f) => &mut f.revision,
                RuleFrame::ResolutionGraveyardOrder(f) => &mut f.revision,
                RuleFrame::OptionalEffect(f) => &mut f.revision,
                RuleFrame::ResolutionPayment(f) => &mut f.revision,
                RuleFrame::TimestampOrder(f) => &mut f.revision,
                RuleFrame::Checks(
                    CheckDecision::CleanupDiscard(f) | CheckDecision::CleanupSelect(f),
                ) => &mut f.revision,
                RuleFrame::Checks(CheckDecision::GraveyardOrder(f)) => &mut f.revision,
                RuleFrame::Checks(CheckDecision::Legend(f)) => &mut f.revision,
                RuleFrame::Triggers(f) => &mut f.revision,
                RuleFrame::TriggerTarget(f) => &mut f.revision,
                RuleFrame::CopyTargets(f) => &mut f.revision,
                RuleFrame::Damage(DamageDecision::Order(f)) => &mut f.revision,
                RuleFrame::Damage(DamageDecision::Allocation(f)) => &mut f.revision,
                RuleFrame::ManaColor(f) => &mut f.revision,
                RuleFrame::Combat(CombatDecision::Attackers(f)) => &mut f.revision,
                RuleFrame::Combat(CombatDecision::Blockers(f)) => &mut f.revision,
                RuleFrame::Combat(CombatDecision::DamageAssignments(f)) => &mut f.revision,
                RuleFrame::CreatureType(f) => &mut f.revision,
            })
        }
        if self == other {
            return true;
        }
        if std::mem::discriminant(self) != std::mem::discriminant(other) {
            return false;
        }
        let mut left = self.clone();
        let mut right = other.clone();
        if let (Some(a), Some(b)) = (issuance_revision(&mut left), issuance_revision(&mut right)) {
            *a = *b;
            left == right
        } else {
            false
        }
    }
    fn effect_decision(&self) -> Option<EffectDecision> {
        match self {
            Self::ReplacementChooser(f) => Some(EffectDecision::ReplacementChooser(f.clone())),
            Self::ZoneReplacement(f) => Some(EffectDecision::ZoneReplacement(f.clone())),
            Self::LibraryTop(f) => Some(EffectDecision::LibraryTop(f.clone())),
            Self::ResolutionSelect(f) => Some(EffectDecision::ResolutionSelect(f.clone())),
            Self::ResolutionGraveyardOrder(f) => {
                Some(EffectDecision::ResolutionGraveyardOrder(f.clone()))
            }
            Self::OptionalEffect(f) => Some(EffectDecision::OptionalEffect(f.clone())),
            Self::ResolutionPayment(f) => Some(EffectDecision::ResolutionPayment(f.clone())),
            Self::TimestampOrder(f) => Some(EffectDecision::TimestampOrder(f.clone())),
            Self::Checks(f) => Some(EffectDecision::Checks(f.clone())),
            Self::Triggers(f) => Some(EffectDecision::Triggers(f.clone())),
            Self::TriggerTarget(f) => Some(EffectDecision::TriggerTarget(f.clone())),
            Self::CopyTargets(f) => Some(EffectDecision::CopyTargets(f.clone())),
            Self::Damage(f) => Some(EffectDecision::Damage(f.clone())),
            Self::Combat(f) => Some(EffectDecision::Combat(f.clone())),
            Self::CreatureType(f) => Some(EffectDecision::CreatureType(f.clone())),
            Self::Opening(_) | Self::ManaColor(_) => None,
        }
    }
}
impl From<EffectDecision> for RuleFrame {
    fn from(frame: EffectDecision) -> Self {
        match frame {
            EffectDecision::ReplacementChooser(f) => Self::ReplacementChooser(f),
            EffectDecision::ZoneReplacement(f) => Self::ZoneReplacement(f),
            EffectDecision::LibraryTop(f) => Self::LibraryTop(f),
            EffectDecision::ResolutionSelect(f) => Self::ResolutionSelect(f),
            EffectDecision::ResolutionGraveyardOrder(f) => Self::ResolutionGraveyardOrder(f),
            EffectDecision::OptionalEffect(f) => Self::OptionalEffect(f),
            EffectDecision::ResolutionPayment(f) => Self::ResolutionPayment(f),
            EffectDecision::TimestampOrder(f) => Self::TimestampOrder(f),
            EffectDecision::Checks(f) => Self::Checks(f),
            EffectDecision::Triggers(f) => Self::Triggers(f),
            EffectDecision::TriggerTarget(f) => Self::TriggerTarget(f),
            EffectDecision::CopyTargets(f) => Self::CopyTargets(f),
            EffectDecision::Damage(f) => Self::Damage(f),
            EffectDecision::Combat(f) => Self::Combat(f),
            EffectDecision::CreatureType(f) => Self::CreatureType(f),
        }
    }
}

/// Privileged native decision record, not a safe public DecisionView.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecisionState {
    id: DecisionId,
    basis: GameBasis,
    chooser: SeatId,
    frame: RuleFrame,
}
impl DecisionState {
    pub fn id(&self) -> DecisionId {
        self.id
    }
    pub fn basis(&self) -> &GameBasis {
        &self.basis
    }
    pub fn chooser(&self) -> SeatId {
        self.chooser
    }
    pub fn frame(&self) -> &RuleFrame {
        &self.frame
    }
}

/// Only commands supported by the current closed-world executor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GameCommand {
    /// Concede as the acting seat. Multiplayer continuation is bounded by its
    /// admitted departure owners; the last remaining opponent wins immediately.
    Concede,
    CastUntargeted {
        spell: ObjectId,
        payment: Option<ManaPayment>,
    },
    PlayLand {
        land: ObjectId,
    },
    ActivateAbility {
        source: ObjectId,
        ability: ActivatedAbilityId,
        target: TargetRef,
        payment: Option<ManaPayment>,
    },
    ActivateMana {
        source: ObjectId,
        ability: crate::mana::ManaAbilityRef,
    },
    Cast {
        spell: ObjectId,
        target: TargetRef,
    },
    CastPaid {
        spell: ObjectId,
        target: TargetRef,
        payment: ManaPayment,
    },
    Pass,
    Reply {
        decision: DecisionId,
        choice: DecisionReply,
    },
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GameInput {
    pub basis: GameBasis,
    pub actor: SeatId,
    pub command: GameCommand,
}
/// Prepared ordinary individual-turn opening after commander/companion setup.
/// No seeded mana, hidden-view contract or full deck legality is implied.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpeningSetup {
    pub seed: GameSeed,
    pub entropy: EntropyTape,
}
enum InitialBoundary {
    Interaction,
    Turn(TurnScheduler),
    Opening(EntropyTape),
}

/// Initialization does not pretend that a previous accepted state existed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CandidateBase {
    Initialize { game: GameId, context: GameContext },
    Advance(GameBasis),
}
/// The caller must still serialize and atomically accept this native candidate.
/// It is not a complete event/replay journal or an authorization receipt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeCandidate {
    base: CandidateBase,
    next: GameState,
    facts: TransitionFacts,
}
/// Current-candidate step boundaries, not an archival or seat-safe replay journal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TurnTransition {
    pub from: Option<TurnPosition>,
    pub to: TurnPosition,
    /// Historical turn owner before entry, even if that owner has left.
    pub active_before: SeatId,
    /// Historical turn owner after entry; current participation is separate.
    pub active_after: SeatId,
}
impl NativeCandidate {
    /// Current accepted check choices and physical discard/SBA moves only.
    pub fn check_journal(&self) -> &CheckJournal {
        &self.facts.check_journal
    }
    /// Declaration facts produced only by this successful candidate.
    pub fn combat_declarations(&self) -> &[CombatDeclarationRecord] {
        &self.facts.combat_declarations
    }
    /// Assignment facts produced only by this successful candidate.
    pub fn combat_assignments(&self) -> &[CombatAssignmentRecord] {
        &self.facts.combat_assignments
    }
    pub fn permanent_entry(&self) -> Option<&PermanentEntryRecord> {
        self.facts.permanent_entry.as_ref()
    }
    pub fn land_play(&self) -> Option<&LandPlayRecord> {
        self.facts.land_play.as_ref()
    }
    pub fn shuffle_journal(&self) -> &[ShuffleRecord] {
        &self.facts.shuffle_journal
    }
    pub fn rules_journal(&self) -> &RulesJournal {
        &self.facts.rules_journal
    }
    pub fn draw_journal(&self) -> &DrawJournal {
        &self.facts.draw_journal
    }
    pub fn turn_transitions(&self) -> &[TurnTransition] {
        &self.facts.turn_transitions
    }
    pub fn ability_activation(&self) -> Option<&AbilityActivationRecord> {
        self.facts.ability_activation.as_ref()
    }
    pub fn mana_production(&self) -> Option<&ManaProductionRecord> {
        self.facts.mana_production.as_ref()
    }
    pub fn mana_payment(&self) -> Option<&ManaPaymentRecord> {
        self.facts.mana_payment.as_ref()
    }
    pub fn base(&self) -> &CandidateBase {
        &self.base
    }
    pub fn damage_journal(&self) -> &DamageJournal {
        &self.facts.damage_journal
    }
    pub fn state(&self) -> &GameState {
        &self.next
    }
    /// Adopt for native execution only; this method performs no durable acceptance.
    pub fn into_state(self) -> GameState {
        self.next
    }
}

/// One root owns one executor and its accepted boundary/continuation metadata.
/// Objects, stack, payloads and resources are never duplicated in another store.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GameState {
    basis: GameBasis,
    execution: EffectWindow,
    original_participants: Vec<SeatId>,
    pending: Option<DecisionState>,
    next_decision: u64,
    turn: Option<TurnScheduler>,
    opening: Option<OpeningState>,
    random: Option<EntropyTape>,
    conclusion: Option<GameConclusion>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GameError {
    Opening(OpeningError),
    Entropy(EntropyError),
    GameEnded,
    /// The current multiplayer cut requires an unimplemented departure owner.
    UnsupportedConcessionScope,
    /// Whole-match bank expiry currently requires an originally two-seat game.
    UnsupportedBankExpiryScope,
    /// The expired participant does not own the current native wait.
    BankPlayerNotWaiting,
    /// Response timeout belongs to originally multiplayer games.
    UnsupportedResponseExpiryScope,
    /// The named participant does not own this revision's response.
    ResponsePlayerNotWaiting,
    PlayerEliminationRequired,
    TurnWorkRequired(TurnWork),
    Turn(TurnError),
    InvalidContext,
    WrongGame,
    WrongContext,
    WrongEpoch,
    StaleRevision,
    UnknownActor,
    DecisionRequired,
    NoDecision,
    WrongDecision,
    WrongChooser,
    RevisionExhausted,
    DecisionIdentityExhausted,
    UnexpectedKernelPhase,
    Effect(EffectError),
}
impl From<OpeningError> for GameError {
    fn from(value: OpeningError) -> Self {
        Self::Opening(value)
    }
}
impl From<EntropyError> for GameError {
    fn from(value: EntropyError) -> Self {
        Self::Entropy(value)
    }
}
impl From<TurnError> for GameError {
    fn from(value: TurnError) -> Self {
        Self::Turn(value)
    }
}
impl From<EffectError> for GameError {
    fn from(value: EffectError) -> Self {
        Self::Effect(value)
    }
}

impl GameState {
    /// Root counter fault injection, absent from production builds.
    #[cfg(test)]
    pub(crate) fn set_test_input_revision(&mut self, revision: u64) {
        self.basis.revision = revision;
        if let Some(pending) = &mut self.pending {
            pending.basis = self.basis.clone();
        }
    }

    /// Decision allocator fault injection, absent from production builds.
    #[cfg(test)]
    pub(crate) fn set_test_next_decision(&mut self, sequence: u64) {
        self.next_decision = sequence;
    }

    /// Private operational counter fork of an actually reached check cut.
    #[cfg(test)]
    pub(crate) fn set_test_check_effect_fault(
        &mut self,
        fault: crate::effects::private_qualification::QualificationFault,
    ) {
        self.execution.qualification_set_fault(fault);
    }

    /// Privileged current allocator counter; actual decisions come from bound receipts.
    pub fn next_decision_sequence(&self) -> u64 {
        self.next_decision
    }

    /// Original ordered native identities, retained independently from live seats.
    /// Historical bindings and original multiplayer policy use this namespace;
    /// current action/priority authority still belongs to the live execution.
    pub fn original_participants(&self) -> &[SeatId] {
        &self.original_participants
    }

    pub fn initialize(
        game: GameId,
        context: GameContext,
        seed: GameSeed,
    ) -> Result<NativeCandidate, GameError> {
        Self::initialize_boundary(game, context, seed, InitialBoundary::Interaction)
    }
    /// Prepared native turn fixture. Executes supported turn work; missing combat
    /// history suspends. Ordinary cleanup uses exact native decisions. This is not
    /// opening setup or restored state.
    pub fn initialize_scheduled(
        game: GameId,
        context: GameContext,
        seed: GameSeed,
        start: TurnStart,
    ) -> Result<NativeCandidate, GameError> {
        let scheduler = TurnScheduler::new(start, seed.seats.len() == 2)?;
        Self::initialize_boundary(game, context, seed, InitialBoundary::Turn(scheduler))
    }
    pub fn initialize_opening(
        game: GameId,
        context: GameContext,
        setup: OpeningSetup,
    ) -> Result<NativeCandidate, GameError> {
        let unique: BTreeSet<_> = setup.seed.seats.iter().copied().collect();
        if !setup.seed.mana.is_empty()
            || !(2..=6).contains(&setup.seed.seats.len())
            || unique.len() != setup.seed.seats.len()
            || !unique.contains(&setup.seed.active)
        {
            return Err(OpeningError::InvalidSetup.into());
        }
        let mut counts = BTreeMap::<SeatId, usize>::new();
        for card in &setup.seed.cards {
            if !unique.contains(&card.owner) {
                return Err(OpeningError::InvalidSetup.into());
            }
            match card.destination {
                crate::object_identity::ObjectDestination::LibraryTop
                | crate::object_identity::ObjectDestination::LibraryBottom => {
                    *counts.entry(card.owner).or_default() += 1
                }
                crate::object_identity::ObjectDestination::Command => {}
                _ => return Err(OpeningError::InvalidSetup.into()),
            }
        }
        if setup
            .seed
            .seats
            .iter()
            .any(|seat| counts.get(seat).copied().unwrap_or(0) < HAND_SIZE)
        {
            return Err(OpeningError::InvalidSetup.into());
        }
        Self::initialize_boundary(
            game,
            context,
            setup.seed,
            InitialBoundary::Opening(setup.entropy),
        )
    }
    fn initialize_boundary(
        game: GameId,
        context: GameContext,
        seed: GameSeed,
        boundary: InitialBoundary,
    ) -> Result<NativeCandidate, GameError> {
        if context.creature_vocabulary != seed.creature_vocabulary.id()
            || context.rules != seed.creature_vocabulary.rules_revision()
            || context.name_rules != seed.name_rules.id()
        {
            return Err(GameError::InvalidContext);
        }
        let base = CandidateBase::Initialize {
            game,
            context: context.clone(),
        };
        let mut facts = TransitionFacts::default();
        let original_participants = seed.seats.clone();
        let multiplayer = original_participants.len() > 2;
        let active = seed.active;
        let (turn, opening, random) = match boundary {
            InitialBoundary::Interaction => (None, None, None),
            InitialBoundary::Turn(turn) => (Some(turn), None, None),
            InitialBoundary::Opening(random) => {
                (None, Some(OpeningState::new(multiplayer)), Some(random))
            }
        };
        let mut execution = EffectWindow::new(seed)?;
        if opening.is_some() {
            execution = execution.begin_opening(execution.revision())?;
        } else if let Some(schedule) = &turn {
            let transition = execution.enter_turn_step(
                execution.revision(),
                active,
                schedule.stage() == TurnStage::Priority,
                false,
            )?;
            let (state, (), append) = transition.into_parts();
            facts.rules_journal.append(append);
            execution = state;
            if schedule.position().step == Step::Untap {
                execution = execution.record_turn_start(execution.revision())?;
            }
            facts.turn_transitions.push(TurnTransition {
                from: None,
                to: schedule.position(),
                active_before: active,
                active_after: active,
            });
        }
        let mut next = Self {
            basis: GameBasis {
                game,
                context,
                epoch: 0,
                revision: 0,
            },
            execution,
            original_participants,
            pending: None,
            next_decision: 1,
            turn,
            opening,
            random,
            conclusion: None,
        };
        if next.opening.is_some() {
            let order = next.opening_order()?;
            for &player in &order {
                next.shuffle_opening(player, OpeningShufflePhase::Initial, &mut facts)?;
            }
            for player in order {
                next.draw_opening(player, &mut facts)?;
            }
        }
        let mut next = next.drive(&mut facts)?;
        next.finalize_checked_losses();
        Ok(NativeCandidate { base, next, facts })
    }
    pub fn basis(&self) -> &GameBasis {
        &self.basis
    }
    /// Current active choice; terminal roots retain only inactive evidence.
    pub fn pending_decision(&self) -> Option<&DecisionState> {
        if self.conclusion.is_some() {
            None
        } else {
            self.pending.as_ref()
        }
    }
    /// Original inactive decision retained after game end; never rebound as a capability.
    pub fn retained_terminal_decision(&self) -> Option<&DecisionState> {
        self.conclusion.as_ref().and(self.pending.as_ref())
    }
    /// Exact root-owned result/cause, available only after actual game conclusion.
    pub fn conclusion(&self) -> Option<&GameConclusion> {
        self.conclusion.as_ref()
    }
    /// Privileged native observation, not a seat-safe projection or mutable handle.
    pub fn execution(&self) -> &EffectWindow {
        &self.execution
    }
    pub fn turn(&self) -> Option<&TurnScheduler> {
        self.turn.as_ref()
    }
    pub fn opening(&self) -> Option<&OpeningState> {
        self.opening.as_ref()
    }
    pub fn entropy(&self) -> Option<&EntropyTape> {
        self.random.as_ref()
    }
    /// Active scheduler work; inspect the retained turn for historical terminal work.
    pub fn pending_turn_work(&self) -> Option<TurnWork> {
        if self.conclusion.is_some() {
            return None;
        }
        match self.turn.as_ref().map(TurnScheduler::stage) {
            Some(TurnStage::Work(work)) => Some(work),
            _ => None,
        }
    }
    /// Restricted originally two-player result from the common root conclusion.
    /// Other winning conditions and multiplayer departure remain unimplemented.
    pub fn outcome(&self) -> Option<GameOutcome> {
        self.conclusion.as_ref().map(GameConclusion::outcome)
    }
    /// This is a blocked rules boundary, not a completed multiplayer outcome.
    pub fn pending_eliminations(&self) -> Vec<SeatId> {
        if self.original_participants.len() > 2 {
            self.execution
                .state_based_losses()
                .iter()
                .copied()
                .collect()
        } else {
            vec![]
        }
    }

    /// Tentatively apply one input, preserving the accepted root on rejection.
    ///
    /// # Errors
    /// Validates basis, actor and terminal status before command-specific work.
    /// Concession additionally rejects root revision exhaustion or unsupported
    /// original participant scope, without consulting priority or a pending chooser.
    pub fn advance(&self, input: GameInput) -> Result<NativeCandidate, GameError> {
        self.validate_basis(&input.basis)?;
        if !self
            .execution
            .window()
            .participants()
            .contains(&input.actor)
        {
            return Err(GameError::UnknownActor);
        }
        if self.outcome().is_some() {
            return Err(GameError::GameEnded);
        }
        if matches!(input.command, GameCommand::Concede) {
            return self.concede(input.actor);
        }
        if !self.execution.state_based_losses().is_empty() {
            return Err(GameError::PlayerEliminationRequired);
        }
        let revision = self
            .basis
            .revision
            .checked_add(1)
            .ok_or(GameError::RevisionExhausted)?;
        if let Some(work) = self.pending_turn_work() {
            let work_reply = matches!(&input.command, GameCommand::Reply { .. })
                && self
                    .pending
                    .as_ref()
                    .is_some_and(|pending| self.frame_matches_turn_work(&pending.frame, work));
            if !work_reply {
                return Err(GameError::TurnWorkRequired(work));
            }
        }
        let mut next = self.clone();
        let mut facts = TransitionFacts::default();
        let mut mana_progress = None;
        let progress = match (&self.pending, input.command) {
            (Some(pending), GameCommand::Reply { decision, choice }) => {
                if decision != pending.id {
                    return Err(GameError::WrongDecision);
                }
                if input.actor != pending.chooser {
                    return Err(GameError::WrongChooser);
                }
                if let RuleFrame::Opening(frame) = &pending.frame {
                    next.reply_opening(frame, choice)?;
                    next.pending = None;
                    None
                } else if let RuleFrame::ManaColor(frame) = &pending.frame {
                    let DecisionReply::ManaColor(color) = choice else {
                        return Err(EffectError::WrongDecisionKind.into());
                    };
                    let completed = self.execution.resume_mana_color(
                        self.execution.revision(),
                        frame,
                        input.actor,
                        color,
                    )?;
                    mana_progress = Some(
                        completed
                            .map_result(|production| ManaActivationResult::Complete { production }),
                    );
                    None
                } else {
                    Some(
                        self.execution.resume_rule(
                            self.execution.revision(),
                            &pending
                                .frame
                                .effect_decision()
                                .ok_or(EffectError::WrongDecisionKind)?,
                            input.actor,
                            choice,
                            self.turn.as_ref(),
                        )?,
                    )
                }
            }
            (Some(pending), GameCommand::ActivateMana { source, ability })
                if matches!(pending.frame, RuleFrame::ResolutionPayment(_)) =>
            {
                if input.actor != pending.chooser {
                    return Err(GameError::WrongChooser);
                }
                mana_progress = Some(self.execution.begin_mana_activation(
                    self.execution.revision(),
                    input.actor,
                    source,
                    ability,
                )?);
                None
            }
            (None, GameCommand::Concede) => return Err(GameError::UnexpectedKernelPhase),
            (Some(_), _) => return Err(GameError::DecisionRequired),
            (None, GameCommand::Reply { .. }) => return Err(GameError::NoDecision),
            (None, GameCommand::Pass) => {
                next.execution = self
                    .execution
                    .pass(self.execution.revision(), input.actor)?;
                None
            }
            (None, GameCommand::PlayLand { land }) => {
                let turn = self
                    .turn
                    .as_ref()
                    .ok_or(EffectError::Timing(TimingError::TurnRequired))?;
                let schedule = turn.record_land_play()?;
                let transition =
                    self.execution
                        .play_land(self.execution.revision(), input.actor, land, turn)?;
                let record = next.accept_execution(transition, &mut facts);
                next.turn = Some(schedule);
                facts.land_play = Some(record);
                None
            }
            (
                None,
                GameCommand::ActivateAbility {
                    source,
                    ability,
                    target,
                    payment,
                },
            ) => {
                let activated = self.execution.activate_ability(
                    self.execution.revision(),
                    input.actor,
                    source,
                    ability,
                    target,
                    payment,
                )?;
                let result = next.accept_execution(activated, &mut facts);
                facts.mana_payment = Some(result.payment);
                facts.ability_activation = Some(result.activation);
                None
            }
            (None, GameCommand::ActivateMana { source, ability }) => {
                mana_progress = Some(self.execution.begin_mana_activation(
                    self.execution.revision(),
                    input.actor,
                    source,
                    ability,
                )?);
                None
            }
            (None, GameCommand::Cast { spell, target }) => {
                let cast = self.execution.cast_on_turn(
                    self.execution.revision(),
                    input.actor,
                    spell,
                    Some(target),
                    None,
                    self.turn.as_ref(),
                )?;
                let payment = next.accept_execution(cast, &mut facts);
                facts.mana_payment = Some(payment);
                None
            }
            (
                None,
                GameCommand::CastPaid {
                    spell,
                    target,
                    payment,
                },
            ) => {
                let cast = self.execution.cast_on_turn(
                    self.execution.revision(),
                    input.actor,
                    spell,
                    Some(target),
                    Some(payment),
                    self.turn.as_ref(),
                )?;
                let payment = next.accept_execution(cast, &mut facts);
                facts.mana_payment = Some(payment);
                None
            }
            (None, GameCommand::CastUntargeted { spell, payment }) => {
                let cast = self.execution.cast_on_turn(
                    self.execution.revision(),
                    input.actor,
                    spell,
                    None,
                    payment,
                    self.turn.as_ref(),
                )?;
                let payment = next.accept_execution(cast, &mut facts);
                facts.mana_payment = Some(payment);
                None
            }
        };
        next.basis.revision = revision;
        if let Some(progress) = progress {
            next.accept_rule_progress(progress, &mut facts)?;
        }
        if let Some(progress) = mana_progress {
            next.accept_mana_progress(progress, &mut facts)?;
        }
        let mut next = next.drive(&mut facts)?;
        next.finalize_checked_losses();
        Ok(NativeCandidate {
            base: CandidateBase::Advance(self.basis.clone()),
            next,
            facts,
        })
    }
    fn validate_basis(&self, basis: &GameBasis) -> Result<(), GameError> {
        if basis.game != self.basis.game {
            return Err(GameError::WrongGame);
        }
        if basis.context != self.basis.context {
            return Err(GameError::WrongContext);
        }
        if basis.epoch != self.basis.epoch {
            return Err(GameError::WrongEpoch);
        }
        if basis.revision != self.basis.revision {
            return Err(GameError::StaleRevision);
        }
        Ok(())
    }
    fn frame_matches_turn_work(&self, frame: &RuleFrame, work: TurnWork) -> bool {
        match (frame, work) {
            (RuleFrame::ZoneReplacement(frame), TurnWork::Cleanup)
                if self.execution.has_check_replacement_work() =>
            {
                matches!(frame.cause(),crate::occurrence::EventCause::CleanupDiscard {boundary:CheckBoundary::Cleanup(position),..}|crate::occurrence::EventCause::StateBasedActions {boundary:CheckBoundary::Cleanup(position),..} if self.turn.as_ref().is_some_and(|turn|turn.position()==*position))
            }
            (RuleFrame::Checks(frame), TurnWork::Cleanup) => {
                matches!(frame.boundary(), CheckBoundary::Cleanup(position)
                    if self.turn.as_ref().is_some_and(|turn| turn.position() == position))
            }
            (RuleFrame::Combat(CombatDecision::Attackers(_)), TurnWork::DeclareAttackers)
            | (RuleFrame::Combat(CombatDecision::Blockers(_)), TurnWork::DeclareBlockers) => true,
            (
                RuleFrame::Combat(CombatDecision::DamageAssignments(frame)),
                TurnWork::CombatDamage { additional: false },
            ) => self
                .turn
                .as_ref()
                .is_some_and(|turn| frame.position() == turn.position()),
            (RuleFrame::Damage(frame), TurnWork::CombatDamage { additional: false }) => {
                matches!(frame.origin(), DamageOrigin::Combat { position }
                    if self.turn.as_ref().is_some_and(|turn| *position == turn.position()))
            }
            _ => false,
        }
    }
    /// Adopt every child append before the next owning operation or scheduler work.
    fn accept_execution<R>(
        &mut self,
        transition: RulesTransition<EffectWindow, R>,
        facts: &mut TransitionFacts,
    ) -> R {
        let (state, result, append) = transition.into_parts();
        facts.rules_journal.append(append);
        self.execution = state;
        result
    }
    fn accept_rule_progress(
        &mut self,
        progress: RuleProgress,
        facts: &mut TransitionFacts,
    ) -> Result<(), GameError> {
        let (state, result, append) = progress.into_parts();
        facts.rules_journal.append(append);
        match result {
            RuleResult::Complete {
                completion,
                checks,
                draws,
                journal,
                declarations,
                assignments,
                permanent_entry: entry,
            } => {
                facts.check_journal.append(checks);
                facts.draw_journal.append(draws);
                facts.damage_journal.append(journal);
                facts.combat_declarations.extend(declarations);
                facts.combat_assignments.extend(assignments);
                let schedule = match completion {
                    RuleCompletion::Checks(completed) => {
                        if entry.is_some() {
                            return Err(GameError::UnexpectedKernelPhase);
                        }
                        match completed.boundary() {
                            CheckBoundary::Priority => None,
                            CheckBoundary::Cleanup(position) => {
                                let turn =
                                    self.turn.as_ref().ok_or(GameError::UnexpectedKernelPhase)?;
                                if turn.position() != position
                                    || turn.stage() != TurnStage::Work(TurnWork::Cleanup)
                                {
                                    return Err(GameError::UnexpectedKernelPhase);
                                }
                                if state.state_based_losses().is_empty() {
                                    Some(turn.complete_work(WorkCompleted::Cleanup {
                                        priority_required: completed.performed()
                                            || completed.waiting_triggers(),
                                    })?)
                                } else {
                                    None
                                }
                            }
                        }
                    }
                    RuleCompletion::Resolution => None,
                    RuleCompletion::TurnWork(work) => {
                        if entry.is_some() {
                            return Err(GameError::UnexpectedKernelPhase);
                        }
                        Some(
                            self.turn
                                .as_ref()
                                .ok_or(GameError::UnexpectedKernelPhase)?
                                .complete_work(work)?,
                        )
                    }
                };
                if entry.is_some() {
                    if facts.permanent_entry.is_some() {
                        return Err(GameError::UnexpectedKernelPhase);
                    }
                    facts.permanent_entry = entry;
                }
                self.execution = state;
                self.pending = None;
                if let Some(schedule) = schedule {
                    self.accept_schedule(schedule, facts)?;
                }
            }
            RuleResult::Pending {
                frame,
                checks,
                draws,
                journal,
                declarations,
                assignments,
            } => {
                facts.check_journal.append(checks);
                facts.draw_journal.append(draws);
                facts.damage_journal.append(journal);
                facts.combat_declarations.extend(declarations);
                facts.combat_assignments.extend(assignments);
                self.bind_decision(frame.into())?;
                self.execution = state;
            }
        }
        Ok(())
    }
    fn bind_decision(&mut self, frame: RuleFrame) -> Result<(), GameError> {
        if self.conclusion.is_some() {
            return Err(GameError::GameEnded);
        }
        let id = DecisionId(self.next_decision);
        self.next_decision = self
            .next_decision
            .checked_add(1)
            .ok_or(GameError::DecisionIdentityExhausted)?;
        self.pending = Some(DecisionState {
            id,
            basis: self.basis.clone(),
            chooser: frame.chooser(),
            frame,
        });
        Ok(())
    }
    fn accept_mana_progress(
        &mut self,
        progress: ManaActivationProgress,
        facts: &mut TransitionFacts,
    ) -> Result<(), GameError> {
        let (state, result, append) = progress.into_parts();
        facts.rules_journal.append(append);
        match result {
            ManaActivationResult::Complete { production } => {
                self.execution = state;
                self.pending = None;
                facts.mana_production = Some(production);
            }
            ManaActivationResult::Pending { frame } => {
                self.bind_decision(RuleFrame::ManaColor(frame))?;
                self.execution = state;
            }
        }
        Ok(())
    }
    fn drive(mut self, facts: &mut TransitionFacts) -> Result<Self, GameError> {
        if self.conclusion.is_some() {
            return Err(GameError::GameEnded);
        }
        loop {
            if self.pending.is_some() || !self.execution.state_based_losses().is_empty() {
                self.finalize_checked_losses();
                return Ok(self);
            }
            if let Some(frame) = self.execution.pending_resolution_decision() {
                self.bind_decision(frame.into())?;
                return Ok(self);
            }
            if self
                .opening
                .as_ref()
                .is_some_and(|opening| !opening.complete())
            {
                self.drive_opening(facts)?;
                continue;
            }
            match self.execution.window().phase() {
                WindowPhase::Priority(PriorityPhase::RequiresChecks { .. }) => {
                    let progress = self.execution.begin_checks(self.execution.revision())?;
                    self.accept_rule_progress(progress, facts)?;
                }
                WindowPhase::CheckingTargets(_) | WindowPhase::Resolving(_) => {
                    let progress = self.execution.begin_resolution(self.execution.revision())?;
                    self.accept_rule_progress(progress, facts)?;
                }
                WindowPhase::StepEnded if self.turn.is_some() => {
                    let turn = self.turn.as_ref().ok_or(GameError::UnexpectedKernelPhase)?;
                    let completed = if turn.position().step == Step::DeclareAttackers {
                        PriorityCompleted::Attackers {
                            declared_or_put_attacking: match self.execution.combat() {
                                Some(combat) => combat.had_attackers(),
                                None if self.execution.window().active_player().is_none() => false,
                                None => return Err(GameError::UnexpectedKernelPhase),
                            },
                        }
                    } else {
                        PriorityCompleted::Ordinary
                    };
                    let next = turn.complete_priority(completed)?;
                    self.accept_schedule(next, facts)?;
                }
                WindowPhase::TurnWork => {
                    let Some(work) = self.pending_turn_work() else {
                        return Err(GameError::UnexpectedKernelPhase);
                    };
                    let result = match work {
                        TurnWork::Untap => {
                            let turn =
                                self.turn.as_ref().ok_or(GameError::UnexpectedKernelPhase)?;
                            let transition = self
                                .execution
                                .untap_on_turn(self.execution.revision(), turn)?;
                            self.accept_execution(transition, facts);
                            WorkCompleted::Untap
                        }
                        TurnWork::Draw => {
                            if self.execution.window().active_player().is_none() {
                                let next = self
                                    .turn
                                    .as_ref()
                                    .ok_or(GameError::UnexpectedKernelPhase)?
                                    .complete_work(WorkCompleted::Draw)?;
                                self.accept_schedule(next, facts)?;
                                continue;
                            }
                            let turn =
                                self.turn.as_ref().ok_or(GameError::UnexpectedKernelPhase)?;
                            let transition = self.execution.draw_cards_with_origin(
                                self.execution.revision(),
                                self.execution.window().turn_owner(),
                                1.into(),
                                DrawOrigin::Turn {
                                    position: turn.position(),
                                },
                            )?;
                            let journal = self.accept_execution(transition, facts);
                            facts.draw_journal.append(journal);
                            WorkCompleted::Draw
                        }
                        TurnWork::Cleanup => {
                            let turn =
                                self.turn.as_ref().ok_or(GameError::UnexpectedKernelPhase)?;
                            let progress = self
                                .execution
                                .begin_cleanup(self.execution.revision(), turn)?;
                            self.accept_rule_progress(progress, facts)?;
                            continue;
                        }
                        TurnWork::DeclareAttackers | TurnWork::DeclareBlockers => {
                            if work == TurnWork::DeclareAttackers
                                && self.execution.window().active_player().is_none()
                            {
                                let next = self
                                    .turn
                                    .as_ref()
                                    .ok_or(GameError::UnexpectedKernelPhase)?
                                    .complete_work(WorkCompleted::DeclareAttackers)?;
                                self.accept_schedule(next, facts)?;
                                continue;
                            }
                            let turn =
                                self.turn.as_ref().ok_or(GameError::UnexpectedKernelPhase)?;
                            let progress = if work == TurnWork::DeclareAttackers {
                                self.execution
                                    .begin_declare_attackers(self.execution.revision(), turn)?
                            } else {
                                if self.execution.combat().is_none() {
                                    return Ok(self);
                                }
                                self.execution
                                    .begin_declare_blockers(self.execution.revision(), turn)?
                            };
                            self.accept_rule_progress(progress, facts)?;
                            continue;
                        }
                        TurnWork::CombatDamage { .. } => {
                            if self.execution.combat().is_none() {
                                return Ok(self);
                            }
                            let turn =
                                self.turn.as_ref().ok_or(GameError::UnexpectedKernelPhase)?;
                            let progress = self
                                .execution
                                .begin_combat_damage(self.execution.revision(), turn)?;
                            self.accept_rule_progress(progress, facts)?;
                            continue;
                        }
                    };
                    let next = self
                        .turn
                        .as_ref()
                        .ok_or(GameError::UnexpectedKernelPhase)?
                        .complete_work(result)?;
                    self.accept_schedule(next, facts)?;
                }
                WindowPhase::Priority(PriorityPhase::Ready { .. }) | WindowPhase::StepEnded => {
                    return Ok(self);
                }
                _ => return Err(GameError::UnexpectedKernelPhase),
            }
        }
    }
    fn accept_schedule(
        &mut self,
        next: TurnScheduler,
        facts: &mut TransitionFacts,
    ) -> Result<(), GameError> {
        let previous_turn = self
            .turn
            .as_ref()
            .ok_or(GameError::UnexpectedKernelPhase)?
            .clone();
        let previous = previous_turn.position();
        let position = next.position();
        let before = self.execution.window().turn_owner();
        let after = if previous.number != position.number {
            let order = self.execution.window().participants();
            let original = self.original_participants();
            let index = original
                .iter()
                .position(|seat| *seat == before)
                .ok_or(GameError::UnexpectedKernelPhase)?;
            original
                .iter()
                .cycle()
                .skip(index + 1)
                .take(original.len())
                .copied()
                .find(|seat| order.contains(seat))
                .ok_or(GameError::UnexpectedKernelPhase)?
        } else {
            before
        };
        if previous.step == Step::EndCombat && previous != position {
            let transition = self
                .execution
                .finish_combat_on_turn(self.execution.revision(), &previous_turn)?;
            self.accept_execution(transition, facts);
        }
        let transition = self.execution.enter_calendar(
            self.execution.revision(),
            Some(&previous_turn),
            &next,
            after,
        )?;
        self.accept_execution(transition, facts);
        if previous != position {
            facts.turn_transitions.push(TurnTransition {
                from: Some(previous),
                to: position,
                active_before: before,
                active_after: after,
            });
        }
        self.turn = Some(next);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::activated::{ActivatedAbility, SourceCost};
    #[test]
    fn draw_failures_after_step_or_card_work_preserve_the_entire_accepted_input() {
        for fault in 0..5 {
            let mut state = GameState::initialize_scheduled(
                GameId(990),
                GameContext::new(
                    "r".into(),
                    "qualification-synthetic".into(),
                    "c".into(),
                    "p".into(),
                    "draw".into(),
                    crate::test_support::creature_types::synthetic_vocabulary().id(),
                    spellwake_contracts::names::NameRules::literal_names().id(),
                )
                .unwrap(),
                GameSeed::with_uniform_life(
                    vec![SeatId(0), SeatId(1)],
                    SeatId(0),
                    vec![SeedCard {
                        card: crate::object_identity::CardId(700),
                        owner: SeatId(0),
                        destination: crate::object_identity::ObjectDestination::LibraryBottom,
                        program: crate::effects::Program::creature(
                            None,
                            1.into(),
                            1.into(),
                            std::collections::BTreeSet::new(),
                        ),
                    }],
                    vec![],
                    vec![FloatingMana {
                        player: SeatId(0),
                        mana_type: crate::mana::ManaType::Blue,
                        amount: 4.into(),
                    }],
                    20.into(),
                    CombatRules::UnrestrictedFreeForAll,
                    crate::test_support::creature_types::synthetic_vocabulary(),
                    std::sync::Arc::new(spellwake_contracts::names::NameRules::literal_names()),
                ),
                TurnStart {
                    number: 2,
                    step: crate::turn::Step::Upkeep,
                },
            )
            .unwrap()
            .into_state();
            state = state
                .advance(GameInput {
                    basis: state.basis.clone(),
                    actor: SeatId(0),
                    command: GameCommand::Pass,
                })
                .unwrap()
                .into_state();
            match fault {
                0 => state.basis.revision = u64::MAX,
                1 => state.execution.set_test_revision(u64::MAX - 3),
                2 => state.execution.set_test_window_revision(u64::MAX - 3),
                3 => state.execution.set_test_object_allocator(u64::MAX, false),
                _ => state.execution.set_test_object_allocator(u64::MAX, true),
            }
            let before = state.clone();
            assert!(
                state
                    .advance(GameInput {
                        basis: state.basis.clone(),
                        actor: SeatId(1),
                        command: GameCommand::Pass
                    })
                    .is_err()
            );
            assert_eq!(state, before);
            assert_eq!(state.execution.window().cards().library(SeatId(0)).len(), 1);
            assert_eq!(
                state.execution.mana(SeatId(0), crate::mana::ManaType::Blue),
                4.into()
            );
        }
    }
    fn failed_draw_fixture(both: bool, cleanup: bool) -> GameState {
        let a = SeatId(0);
        let b = SeatId(1);
        let mut execution = EffectWindow::new(GameSeed::with_uniform_life(
            vec![a, b],
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
        execution = execution
            .enter_turn_step(execution.revision(), a, false, false)
            .unwrap()
            .into_state();
        execution = execution
            .draw_cards(execution.revision(), a, 1.into())
            .unwrap()
            .into_state();
        if both {
            execution = execution
                .draw_cards(execution.revision(), b, 1.into())
                .unwrap()
                .into_state();
        }
        if !cleanup {
            execution = execution
                .enter_turn_step(execution.revision(), a, true, false)
                .unwrap()
                .into_state();
        }
        GameState {
            basis: GameBasis {
                game: GameId(991),
                context: GameContext::new(
                    "r".into(),
                    "qualification-synthetic".into(),
                    "c".into(),
                    "p".into(),
                    "draw".into(),
                    crate::test_support::creature_types::synthetic_vocabulary().id(),
                    spellwake_contracts::names::NameRules::literal_names().id(),
                )
                .unwrap(),
                epoch: 0,
                revision: 0,
            },
            original_participants: execution.window().participants().to_vec(),
            execution,
            pending: None,
            next_decision: 1,
            opening: None,
            random: None,
            conclusion: None,
            turn: if cleanup {
                Some(
                    TurnScheduler::new(
                        TurnStart {
                            number: 3,
                            step: crate::turn::Step::Cleanup,
                        },
                        true,
                    )
                    .unwrap(),
                )
            } else {
                None
            },
        }
    }
    #[test]
    fn simultaneous_empty_draw_losses_make_a_two_player_draw() {
        let state = failed_draw_fixture(true, false)
            .drive(&mut TransitionFacts::default())
            .unwrap();
        assert_eq!(state.outcome(), Some(GameOutcome::Draw));
        assert_eq!(state.execution.state_based_losses().len(), 2);
    }
    #[test]
    fn cleanup_loss_stops_before_turn_rotation_or_priority() {
        let mut facts = TransitionFacts::default();
        let state = failed_draw_fixture(false, true).drive(&mut facts).unwrap();
        assert_eq!(state.outcome(), Some(GameOutcome::Winner(SeatId(1))));
        assert_eq!(
            state.turn().unwrap().position().step,
            crate::turn::Step::Cleanup
        );
        assert!(facts.turn_transitions.is_empty());
        assert_eq!(state.execution.window().turn_owner(), SeatId(0));
        assert_eq!(state.execution.window().phase(), WindowPhase::TurnWork);
    }

    fn scheduled_end(number: u64) -> GameState {
        GameState::initialize_scheduled(
            GameId(700),
            GameContext::new(
                "r".into(),
                "qualification-synthetic".into(),
                "c".into(),
                "p".into(),
                "turn".into(),
                crate::test_support::creature_types::synthetic_vocabulary().id(),
                spellwake_contracts::names::NameRules::literal_names().id(),
            )
            .unwrap(),
            GameSeed::with_uniform_life(
                vec![SeatId(0), SeatId(1)],
                SeatId(0),
                vec![],
                vec![],
                vec![FloatingMana {
                    player: SeatId(0),
                    mana_type: crate::mana::ManaType::Blue,
                    amount: 2.into(),
                }],
                20.into(),
                CombatRules::UnrestrictedFreeForAll,
                crate::test_support::creature_types::synthetic_vocabulary(),
                std::sync::Arc::new(spellwake_contracts::names::NameRules::literal_names()),
            ),
            TurnStart {
                number,
                step: crate::turn::Step::End,
            },
        )
        .unwrap()
        .into_state()
    }

    #[test]
    fn late_step_failure_preserves_accepted_pass_state_and_nonzero_pools() {
        let initial = scheduled_end(2);
        let mut state = initial
            .advance(GameInput {
                basis: initial.basis.clone(),
                actor: SeatId(0),
                command: GameCommand::Pass,
            })
            .unwrap()
            .into_state();
        state.execution.set_test_revision(u64::MAX - 1);
        let before = state.clone();
        assert!(
            state
                .advance(GameInput {
                    basis: state.basis.clone(),
                    actor: SeatId(1),
                    command: GameCommand::Pass
                })
                .is_err()
        );
        assert_eq!(state, before);
        assert_eq!(
            state.execution.mana(SeatId(0), crate::mana::ManaType::Blue),
            2.into()
        );
    }

    #[test]
    fn late_turn_exhaustion_rolls_back_cleanup_and_mana_emptying() {
        let initial = scheduled_end(u64::MAX);
        let state = initial
            .advance(GameInput {
                basis: initial.basis.clone(),
                actor: SeatId(0),
                command: GameCommand::Pass,
            })
            .unwrap()
            .into_state();
        let before = state.clone();
        assert_eq!(
            state.advance(GameInput {
                basis: state.basis.clone(),
                actor: SeatId(1),
                command: GameCommand::Pass
            }),
            Err(GameError::Turn(TurnError::TurnExhausted))
        );
        assert_eq!(state, before);
        assert_eq!(
            state.execution.mana(SeatId(0), crate::mana::ManaType::Blue),
            2.into()
        );
    }
    fn activation_game(source_cost: SourceCost) -> GameState {
        use crate::mana::ManaType;
        let a = SeatId(0);
        let b = SeatId(1);
        GameState::initialize(
            GameId(63),
            GameContext::new(
                "r".into(),
                "qualification-synthetic".into(),
                "cards".into(),
                "programs".into(),
                "activation".into(),
                crate::test_support::creature_types::synthetic_vocabulary().id(),
                spellwake_contracts::names::NameRules::literal_names().id(),
            )
            .unwrap(),
            GameSeed::with_uniform_life(
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
                                mana_cost: crate::mana::ManaCost::one(ManaType::Red),
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
            ),
        )
        .unwrap()
        .into_state()
    }
    fn activation_input(state: &GameState) -> GameInput {
        GameInput {
            basis: state.basis.clone(),
            actor: SeatId(0),
            command: GameCommand::ActivateAbility {
                source: state
                    .execution
                    .window()
                    .cards()
                    .current_identity(CardId(1))
                    .unwrap(),
                ability: ActivatedAbilityId(7),
                target: TargetRef::Object(
                    state
                        .execution
                        .window()
                        .cards()
                        .current_identity(CardId(2))
                        .unwrap(),
                ),
                payment: None,
            },
        }
    }
    #[test]
    fn activated_admission_late_failures_accept_no_cost_payment_or_entry() {
        use crate::mana::ManaType;
        use crate::stack_window::StackWindowError;
        for cost in [SourceCost::Tap, SourceCost::Sacrifice] {
            for fault in 0..5 {
                let mut state = activation_game(cost);
                let source = state
                    .execution
                    .window()
                    .cards()
                    .current_identity(CardId(1))
                    .unwrap();
                let error = match fault {
                    0 => {
                        state.basis.revision = u64::MAX;
                        GameError::RevisionExhausted
                    }
                    1 => {
                        state.execution.set_test_revision(u64::MAX);
                        GameError::Effect(EffectError::Window(StackWindowError::RevisionExhausted))
                    }
                    2 => {
                        state.execution.set_test_revision(u64::MAX - 1);
                        GameError::Effect(EffectError::Window(StackWindowError::RevisionExhausted))
                    }
                    3 => {
                        state.execution.set_test_window_revision(u64::MAX - 1);
                        GameError::Effect(EffectError::Window(StackWindowError::RevisionExhausted))
                    }
                    _ => {
                        state.execution.set_test_priority_revision(u64::MAX - 1);
                        GameError::Effect(EffectError::Window(StackWindowError::Priority(
                            crate::priority::PriorityError::RevisionExhausted,
                        )))
                    }
                };
                let before = state.clone();
                assert_eq!(state.advance(activation_input(&state)), Err(error));
                assert_eq!(state, before);
                assert_eq!(
                    state
                        .execution
                        .window()
                        .cards()
                        .get(source)
                        .unwrap()
                        .tapped(),
                    Some(false)
                );
                assert!(state.execution.window().entries().is_empty());
                assert_eq!(state.execution.mana(SeatId(0), ManaType::Red), 7.into());
                assert_eq!(state.execution.mana(SeatId(1), ManaType::Blue), 11.into());
            }
        }
    }
    #[test]
    fn activated_resolution_failure_keeps_previously_paid_sacrifice_and_stack() {
        use crate::mana::ManaType;
        let initial = activation_game(SourceCost::Sacrifice);
        let paid = initial
            .advance(activation_input(&initial))
            .unwrap()
            .into_state();
        let paid = paid
            .advance(GameInput {
                basis: paid.basis.clone(),
                actor: SeatId(0),
                command: GameCommand::Pass,
            })
            .unwrap()
            .into_state();
        for remaining in [1, 2] {
            let mut state = paid.clone();
            state.execution.set_test_revision(u64::MAX - remaining);
            let before = state.clone();
            let source = state
                .execution
                .window()
                .cards()
                .current_identity(CardId(1))
                .unwrap();
            let target = state
                .execution
                .window()
                .cards()
                .current_identity(CardId(2))
                .unwrap();
            assert_eq!(
                state.advance(GameInput {
                    basis: state.basis.clone(),
                    actor: SeatId(1),
                    command: GameCommand::Pass
                }),
                Err(GameError::Effect(EffectError::Window(
                    crate::stack_window::StackWindowError::RevisionExhausted
                )))
            );
            assert_eq!(state, before);
            assert_eq!(
                state.execution.window().cards().get(source).unwrap().zone(),
                crate::object_identity::ObjectZone::Graveyard(SeatId(1))
            );
            assert_eq!(
                state.execution.creature(target).unwrap().unwrap().damage,
                0.into()
            );
            assert_eq!(state.execution.window().entries().len(), 1);
            assert_eq!(state.execution.mana(SeatId(0), ManaType::Red), 6.into());
            assert_eq!(state.execution.mana(SeatId(1), ManaType::Blue), 11.into());
        }
    }
    fn color_game() -> GameState {
        use crate::mana::{ManaAbility, ManaAbilityId, ManaOutput, ManaType};
        use std::collections::BTreeSet;
        let a = SeatId(0);
        let b = SeatId(1);
        GameState::initialize(
            GameId(62),
            GameContext::new(
                "r".into(),
                "qualification-synthetic".into(),
                "cards".into(),
                "programs".into(),
                "color".into(),
                crate::test_support::creature_types::synthetic_vocabulary().id(),
                spellwake_contracts::names::NameRules::literal_names().id(),
            )
            .unwrap(),
            GameSeed::with_uniform_life(
                vec![a, b],
                a,
                vec![SeedCard {
                    card: CardId(1),
                    owner: b,
                    destination: ObjectDestination::Battlefield { controller: a },
                    program: Program::land(
                        std::collections::BTreeSet::new(),
                        vec![ManaAbility {
                            id: ManaAbilityId(7),
                            output: ManaOutput::ChooseColor {
                                amount: 3.into(),
                                colors: BTreeSet::from([ManaType::Red, ManaType::Green]),
                            },
                        }],
                    ),
                }],
                vec![CardId(1)],
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
            ),
        )
        .unwrap()
        .into_state()
    }
    fn activate_color_input(state: &GameState) -> GameInput {
        GameInput {
            basis: state.basis.clone(),
            actor: SeatId(0),
            command: GameCommand::ActivateMana {
                source: state
                    .execution
                    .window()
                    .cards()
                    .current_identity(CardId(1))
                    .unwrap(),
                ability: crate::mana::ManaAbilityRef::Printed(ManaAbilityId(7)),
            },
        }
    }
    #[test]
    fn color_pending_admission_exhaustion_preserves_unpaid_state_and_basis() {
        use crate::mana::ManaType;
        for fault in 0..3 {
            let mut state = color_game();
            let source = state
                .execution
                .window()
                .cards()
                .current_identity(CardId(1))
                .unwrap();
            let error = match fault {
                0 => {
                    state.basis.revision = u64::MAX;
                    GameError::RevisionExhausted
                }
                1 => {
                    state.execution.set_test_revision(u64::MAX);
                    GameError::Effect(EffectError::Window(
                        crate::stack_window::StackWindowError::RevisionExhausted,
                    ))
                }
                _ => {
                    state.next_decision = u64::MAX;
                    GameError::DecisionIdentityExhausted
                }
            };
            let before = state.clone();
            assert_eq!(state.advance(activate_color_input(&state)), Err(error));
            assert_eq!(state, before);
            assert_eq!(
                state
                    .execution
                    .window()
                    .cards()
                    .get(source)
                    .unwrap()
                    .tapped(),
                Some(false)
            );
            assert_eq!(state.execution.mana(SeatId(0), ManaType::Red), 5.into());
            assert_eq!(state.execution.mana(SeatId(1), ManaType::Blue), 11.into());
        }
        let mut state = color_game();
        state.next_decision = u64::MAX - 1;
        let candidate = state.advance(activate_color_input(&state)).unwrap();
        assert!(candidate.mana_production().is_none());
        assert!(candidate.mana_payment().is_none());
        let paid = candidate.into_state();
        let decision = paid.pending_decision().unwrap();
        assert_eq!(decision.id(), DecisionId(u64::MAX - 1));
        assert_eq!(decision.basis(), paid.basis());
        let complete = paid
            .advance(GameInput {
                basis: paid.basis.clone(),
                actor: SeatId(0),
                command: GameCommand::Reply {
                    decision: decision.id(),
                    choice: DecisionReply::ManaColor(ManaType::Green),
                },
            })
            .unwrap();
        assert!(complete.state().pending_decision().is_none());
        assert_eq!(
            complete
                .mana_production()
                .unwrap()
                .produced
                .get(&ManaType::Green),
            Some(&3.into())
        );
    }
    #[test]
    fn color_final_reply_failures_preserve_committed_tap_without_credit_or_record() {
        use crate::mana::ManaType;
        use crate::stack_window::StackWindowError;
        let initial = color_game();
        let initial = initial
            .advance(activate_color_input(&initial))
            .unwrap()
            .into_state();
        for fault in 0..5 {
            let mut state = initial.clone();
            let error = match fault {
                0 => {
                    state.basis.revision = u64::MAX;
                    GameError::RevisionExhausted
                }
                1 | 2 => {
                    let revision = if fault == 1 { u64::MAX } else { u64::MAX - 1 };
                    let frame = state.execution.set_test_mana_revision(revision);
                    state.pending.as_mut().unwrap().frame = RuleFrame::ManaColor(frame);
                    GameError::Effect(EffectError::Window(StackWindowError::RevisionExhausted))
                }
                3 => {
                    state.execution.set_test_window_revision(u64::MAX);
                    GameError::Effect(EffectError::Window(StackWindowError::RevisionExhausted))
                }
                _ => {
                    state.execution.set_test_priority_revision(u64::MAX);
                    GameError::Effect(EffectError::Window(StackWindowError::Priority(
                        crate::priority::PriorityError::RevisionExhausted,
                    )))
                }
            };
            let source = state
                .execution
                .window()
                .cards()
                .current_identity(CardId(1))
                .unwrap();
            let before = state.clone();
            let decision = state.pending_decision().unwrap().id();
            assert_eq!(
                state.advance(GameInput {
                    basis: state.basis.clone(),
                    actor: SeatId(0),
                    command: GameCommand::Reply {
                        decision,
                        choice: DecisionReply::ManaColor(ManaType::Red)
                    }
                }),
                Err(error)
            );
            assert_eq!(state, before);
            assert_eq!(
                state
                    .execution
                    .window()
                    .cards()
                    .get(source)
                    .unwrap()
                    .tapped(),
                Some(true)
            );
            assert_eq!(state.execution.mana(SeatId(0), ManaType::Red), 5.into());
            assert_eq!(state.execution.mana(SeatId(1), ManaType::Blue), 11.into());
        }
    }
    #[test]
    fn mana_activation_late_drive_failure_accepts_no_tap_credit_or_record() {
        use crate::mana::{ManaAbility, ManaAbilityId, ManaOutput, ManaType};
        use std::collections::BTreeMap;
        let a = SeatId(0);
        let b = SeatId(1);
        let initial = GameState::initialize(
            GameId(60),
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
                vec![SeedCard {
                    card: CardId(1),
                    owner: a,
                    destination: ObjectDestination::Battlefield { controller: a },
                    program: Program::land(
                        std::collections::BTreeSet::new(),
                        vec![ManaAbility {
                            id: ManaAbilityId(1),
                            output: ManaOutput::Fixed(BTreeMap::from([(ManaType::Red, 2.into())])),
                        }],
                    ),
                }],
                vec![CardId(1)],
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
            ),
        )
        .unwrap()
        .into_state();
        let source = initial
            .execution
            .window()
            .cards()
            .current_identity(CardId(1))
            .unwrap();
        for fault in 0..4 {
            let mut state = initial.clone();
            let expected = if fault == 0 {
                state.basis.revision = u64::MAX;
                GameError::RevisionExhausted
            } else {
                match fault {
                    1 => state.execution.set_test_revision(u64::MAX),
                    2 => state.execution.set_test_revision(u64::MAX - 1),
                    _ => state.execution.set_test_window_revision(u64::MAX - 1),
                }
                GameError::Effect(EffectError::Window(
                    crate::stack_window::StackWindowError::RevisionExhausted,
                ))
            };
            let before = state.clone();
            assert_eq!(
                state.advance(GameInput {
                    basis: state.basis.clone(),
                    actor: a,
                    command: GameCommand::ActivateMana {
                        source,
                        ability: crate::mana::ManaAbilityRef::Printed(ManaAbilityId(1))
                    }
                }),
                Err(expected)
            );
            assert_eq!(state, before);
            assert_eq!(state.execution.mana(a, ManaType::Red), 5.into());
            assert_eq!(state.execution.mana(b, ManaType::Blue), 11.into());
            assert_eq!(
                state
                    .execution
                    .window()
                    .cards()
                    .get(source)
                    .unwrap()
                    .tapped(),
                Some(false)
            );
            assert!(state.execution.window().entries().is_empty());
        }
    }
    use crate::{
        effects::{Instruction, Program},
        mana::{ManaCost, ManaType},
        object_identity::{CardId, ObjectDestination},
    };
    #[test]
    fn paid_cast_outer_and_post_cast_drive_overflow_accept_no_receipt_or_debit() {
        let a = SeatId(0);
        let b = SeatId(1);
        let mut state = GameState::initialize(
            GameId(50),
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
                vec![FloatingMana {
                    player: a,
                    mana_type: ManaType::Red,
                    amount: 2.into(),
                }],
                20.into(),
                CombatRules::UnrestrictedFreeForAll,
                crate::test_support::creature_types::synthetic_vocabulary(),
                std::sync::Arc::new(spellwake_contracts::names::NameRules::literal_names()),
            ),
        )
        .unwrap()
        .into_state();
        let spell = state
            .execution
            .window()
            .cards()
            .current_identity(CardId(1))
            .unwrap();
        let target = TargetRef::Object(
            state
                .execution
                .window()
                .cards()
                .current_identity(CardId(2))
                .unwrap(),
        );
        state.basis.revision = u64::MAX;
        let saved = state.clone();
        assert_eq!(
            state.advance(GameInput {
                basis: state.basis.clone(),
                actor: a,
                command: GameCommand::Cast { spell, target }
            }),
            Err(GameError::RevisionExhausted)
        );
        assert_eq!(state, saved);
        state.basis.revision = 0;
        for revision in [u64::MAX, u64::MAX - 1] {
            state.execution.set_test_revision(revision);
            let saved = state.clone();
            assert_eq!(
                state.advance(GameInput {
                    basis: state.basis.clone(),
                    actor: a,
                    command: GameCommand::Cast { spell, target }
                }),
                Err(GameError::Effect(EffectError::Window(
                    crate::stack_window::StackWindowError::RevisionExhausted
                )))
            );
            assert_eq!(state, saved);
            assert_eq!(state.execution.mana(a, ManaType::Red), 2.into());
            assert!(state.execution.window().entries().is_empty());
        }
    }
    #[test]
    fn initial_program_allocation_failure_discards_all_prior_instructions() {
        let a = SeatId(0);
        let b = SeatId(1);
        let mut state = GameState::initialize(
            GameId(40),
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
                    SeedCard {
                        card: CardId(1),
                        owner: a,
                        destination: ObjectDestination::Hand,
                        program: Program::instant(
                            ManaCost::one(ManaType::Red),
                            ExecutableBody::new(
                                BodyTarget::SingleCreature,
                                vec![
                                    Instruction::Damage { amount: 2.into() },
                                    Instruction::CreatePreventionShield { amount: 3.into() },
                                    Instruction::CreatePreventionShield { amount: 4.into() },
                                    Instruction::Damage { amount: 5.into() },
                                ],
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
                            100.into(),
                            std::collections::BTreeSet::new(),
                        ),
                    },
                ],
                vec![CardId(2)],
                vec![FloatingMana {
                    player: a,
                    mana_type: ManaType::Red,
                    amount: 1.into(),
                }],
                20.into(),
                CombatRules::UnrestrictedFreeForAll,
                crate::test_support::creature_types::synthetic_vocabulary(),
                std::sync::Arc::new(spellwake_contracts::names::NameRules::literal_names()),
            ),
        )
        .unwrap()
        .into_state();
        let target = state
            .execution
            .window()
            .cards()
            .current_identity(CardId(2))
            .unwrap();
        let spell = state
            .execution
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
            state = state
                .advance(GameInput {
                    basis: state.basis.clone(),
                    actor,
                    command,
                })
                .unwrap()
                .into_state();
        }
        state.next_decision = u64::MAX;
        let saved = state.clone();
        assert_eq!(
            state.advance(GameInput {
                basis: state.basis.clone(),
                actor: b,
                command: GameCommand::Pass
            }),
            Err(GameError::DecisionIdentityExhausted)
        );
        assert_eq!(state, saved);
        assert_eq!(
            state.execution.creature(target).unwrap().unwrap().damage,
            0.into()
        );
        assert!(state.execution.prevention_shields().next().is_none());
        assert_eq!(state.execution.instruction_index(), None);
        assert!(state.pending.is_none());
    }
    #[test]
    fn allocation_failure_after_a_mixed_reply_preserves_accepted_suspension() {
        let a = SeatId(0);
        let b = SeatId(1);
        let instructions = [
            Instruction::CreatePreventionShield { amount: 1.into() },
            Instruction::CreateDamageMultiplier { factor: 2.into() },
            Instruction::CreateDamageMultiplier { factor: 3.into() },
            Instruction::Damage { amount: 5.into() },
        ];
        let mut cards: Vec<_> = instructions
            .into_iter()
            .enumerate()
            .map(|(index, instruction)| SeedCard {
                card: CardId(index as u64 + 1),
                owner: a,
                destination: ObjectDestination::Hand,
                program: Program::instant(
                    ManaCost::one(ManaType::Red),
                    ExecutableBody::new(BodyTarget::SingleCreature, vec![instruction]),
                ),
            })
            .collect();
        cards.push(SeedCard {
            card: CardId(10),
            owner: b,
            destination: ObjectDestination::Battlefield { controller: b },
            program: Program::creature(
                None,
                1.into(),
                100.into(),
                std::collections::BTreeSet::new(),
            ),
        });
        let context = GameContext::new(
            "test".into(),
            "qualification-synthetic".into(),
            "cards".into(),
            "programs".into(),
            "profile".into(),
            crate::test_support::creature_types::synthetic_vocabulary().id(),
            spellwake_contracts::names::NameRules::literal_names().id(),
        )
        .unwrap();
        let mut state = GameState::initialize(GameId(20), context, {
            // Fixture premise: seeded BF timestamps follow this authored card sequence.
            let initial_battlefield_order = cards
                .iter()
                .filter(|card| {
                    matches!(
                        card.destination,
                        crate::object_identity::ObjectDestination::Battlefield { .. }
                    )
                })
                .map(|card| card.card)
                .collect();
            GameSeed::with_uniform_life(
                vec![a, b],
                a,
                cards,
                initial_battlefield_order,
                vec![FloatingMana {
                    player: a,
                    mana_type: ManaType::Red,
                    amount: 4.into(),
                }],
                20.into(),
                CombatRules::UnrestrictedFreeForAll,
                crate::test_support::creature_types::synthetic_vocabulary(),
                std::sync::Arc::new(spellwake_contracts::names::NameRules::literal_names()),
            )
        })
        .unwrap()
        .into_state();
        for card in 1..=4 {
            let target = state
                .execution
                .window()
                .cards()
                .current_identity(CardId(10))
                .unwrap();
            let spell = state
                .execution
                .window()
                .cards()
                .current_identity(CardId(card))
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
                (b, GameCommand::Pass),
            ] {
                state = state
                    .advance(GameInput {
                        basis: state.basis.clone(),
                        actor,
                        command,
                    })
                    .unwrap()
                    .into_state();
            }
        }
        assert!(state.pending.is_some());
        state.next_decision = u64::MAX;
        let saved = state.clone();
        let shield = state.execution.prevention_shields().next().unwrap().id;
        let pending = state.pending.as_ref().unwrap();
        let RuleFrame::Damage(DamageDecision::Order(frame)) = pending.frame() else {
            panic!("damage ordering expected")
        };
        let application = frame
            .applications()
            .iter()
            .find(|application| application.effect == shield.into())
            .copied()
            .unwrap();
        assert_eq!(
            state.advance(GameInput {
                basis: state.basis.clone(),
                actor: b,
                command: GameCommand::Reply {
                    decision: pending.id,
                    choice: DecisionReply::DamageOrder(application)
                }
            }),
            Err(GameError::DecisionIdentityExhausted)
        );
        assert_eq!(state, saved);
        assert_eq!(
            state
                .execution
                .pending_damage()
                .unwrap()
                .components()
                .next()
                .unwrap()
                .remaining(),
            &5.into()
        );
        assert_eq!(
            state.execution.prevention_shield(shield).unwrap().remaining,
            1.into()
        );
        assert_eq!(state.execution.pending_damage().unwrap().stage(), 0);
    }
    #[test]
    fn context_rejects_missing_or_noncanonical_identity_components() {
        for index in 0..5 {
            for invalid in ["", "   ", " release", "release "] {
                let mut labels =
                    ["release", "rules", "catalogue", "programs", "profile"].map(String::from);
                labels[index] = invalid.into();
                let [release, rules, catalogue, programs, profile] = labels;
                assert_eq!(
                    GameContext::new(
                        release,
                        rules,
                        catalogue,
                        programs,
                        profile,
                        crate::test_support::creature_types::synthetic_vocabulary().id(),
                        spellwake_contracts::names::NameRules::literal_names().id(),
                    ),
                    Err(GameError::InvalidContext)
                );
            }
        }
    }
    #[test]
    fn revision_exhaustion_preserves_private_state_without_forging_a_public_constructor() {
        let context = GameContext::new(
            "r".into(),
            "qualification-synthetic".into(),
            "cards".into(),
            "programs".into(),
            "profile".into(),
            crate::test_support::creature_types::synthetic_vocabulary().id(),
            spellwake_contracts::names::NameRules::literal_names().id(),
        )
        .unwrap();
        let mut game = GameState::initialize(
            GameId(1),
            context,
            GameSeed::with_uniform_life(
                vec![SeatId(0), SeatId(1)],
                SeatId(0),
                vec![],
                vec![],
                vec![],
                20.into(),
                CombatRules::UnrestrictedFreeForAll,
                crate::test_support::creature_types::synthetic_vocabulary(),
                std::sync::Arc::new(spellwake_contracts::names::NameRules::literal_names()),
            ),
        )
        .unwrap()
        .into_state();
        game.basis.revision = u64::MAX;
        let saved = game.clone();
        assert_eq!(
            game.advance(GameInput {
                basis: game.basis.clone(),
                actor: SeatId(0),
                command: GameCommand::Pass
            }),
            Err(GameError::RevisionExhausted)
        );
        assert_eq!(game, saved);
    }

    #[test]
    fn decision_identity_exhaustion_rejects_the_last_pass_without_partial_resolution() {
        let a = SeatId(0);
        let b = SeatId(1);
        let context = GameContext::new(
            "native-test".into(),
            "qualification-synthetic".into(),
            "cards".into(),
            "programs".into(),
            "profile".into(),
            crate::test_support::creature_types::synthetic_vocabulary().id(),
            spellwake_contracts::names::NameRules::literal_names().id(),
        )
        .unwrap();
        let instant = |card, owner, colour, instructions, body_target| SeedCard {
            card: CardId(card),
            owner,
            destination: ObjectDestination::Hand,
            program: Program::instant(
                ManaCost::one(colour),
                ExecutableBody::new(body_target, instructions),
            ),
        };
        let mut game = GameState::initialize(
            GameId(1),
            context,
            GameSeed::with_uniform_life(
                vec![a, b],
                a,
                vec![
                    instant(
                        1,
                        a,
                        ManaType::Red,
                        vec![Instruction::Damage { amount: 3.into() }],
                        BodyTarget::SingleCreature,
                    ),
                    instant(
                        2,
                        b,
                        ManaType::Blue,
                        vec![Instruction::CopySpell, Instruction::CopySpell],
                        BodyTarget::SingleSpell(SpellTargetFilter::CopyableInstructionSpell),
                    ),
                    SeedCard {
                        card: CardId(3),
                        owner: b,
                        destination: ObjectDestination::Battlefield { controller: b },
                        program: Program::creature(
                            None,
                            2.into(),
                            2.into(),
                            std::collections::BTreeSet::new(),
                        ),
                    },
                ],
                vec![CardId(3)],
                vec![
                    FloatingMana {
                        player: a,
                        mana_type: ManaType::Red,
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
        let object = |game: &GameState, card| {
            game.execution
                .window()
                .cards()
                .current_identity(CardId(card))
                .unwrap()
        };
        let command = GameCommand::Cast {
            spell: object(&game, 1),
            target: TargetRef::Object(object(&game, 3)),
        };
        game = game
            .advance(GameInput {
                basis: game.basis.clone(),
                actor: a,
                command,
            })
            .unwrap()
            .into_state();
        game = game
            .advance(GameInput {
                basis: game.basis.clone(),
                actor: a,
                command: GameCommand::Pass,
            })
            .unwrap()
            .into_state();
        let original = game.execution.window().entries()[0].id();
        let command = GameCommand::Cast {
            spell: object(&game, 2),
            target: TargetRef::Stack(original),
        };
        game = game
            .advance(GameInput {
                basis: game.basis.clone(),
                actor: b,
                command,
            })
            .unwrap()
            .into_state();
        game = game
            .advance(GameInput {
                basis: game.basis.clone(),
                actor: b,
                command: GameCommand::Pass,
            })
            .unwrap()
            .into_state();
        game.next_decision = u64::MAX;
        let saved = game.clone();
        assert_eq!(
            game.advance(GameInput {
                basis: game.basis.clone(),
                actor: a,
                command: GameCommand::Pass
            }),
            Err(GameError::DecisionIdentityExhausted)
        );
        assert_eq!(game, saved);
        assert!(game.pending_decision().is_none());
        assert!(game.execution.window().spell_copies().is_empty());
        assert!(
            matches!(game.execution.window().phase(), WindowPhase::Priority(PriorityPhase::Ready { holder }) if holder == a)
        );
        // Accept the first suspension, then fail only after its valid reply creates
        // a copy and the next instruction needs another decision identity.
        game.next_decision = 1;
        let mut pending = game
            .advance(GameInput {
                basis: game.basis.clone(),
                actor: a,
                command: GameCommand::Pass,
            })
            .unwrap()
            .into_state();
        pending.next_decision = u64::MAX;
        let saved = pending.clone();
        let decision = pending.pending.as_ref().unwrap();
        assert_eq!(
            pending.advance(GameInput {
                basis: pending.basis.clone(),
                actor: b,
                command: GameCommand::Reply {
                    decision: decision.id,
                    choice: DecisionReply::CopyTargets(crate::effects::CopyTargetChoice::Retain)
                }
            }),
            Err(GameError::DecisionIdentityExhausted)
        );
        assert_eq!(pending, saved);
        assert!(pending.execution.window().spell_copies().is_empty());
        assert_eq!(pending.execution.instruction_index(), Some(0));
    }
}

#[cfg(test)]
mod combat_tests {
    use super::*;
    use crate::{
        combat::{AttackDeclaration, BlockDeclaration, BlockStatus},
        effects::{Instruction, Program},
        mana::ManaCost,
        object_identity::{CardId, ObjectDestination},
    };
    const A: SeatId = SeatId(17);
    const B: SeatId = SeatId(4);
    const C: SeatId = SeatId(90);
    const D: SeatId = SeatId(2);

    fn fixture(multiplayer: bool) -> GameState {
        let mut cards = vec![];
        for (id, player) in [(1, A), (2, A), (3, B), (4, B), (5, C), (6, D), (7, A)] {
            if !multiplayer && [C, D].contains(&player) {
                continue;
            }
            cards.push(SeedCard {
                card: CardId(id),
                owner: player,
                destination: ObjectDestination::Battlefield { controller: player },
                program: Program::creature(
                    None,
                    2.into(),
                    4.into(),
                    std::collections::BTreeSet::new(),
                ),
            });
        }
        cards.push(SeedCard {
            card: CardId(30),
            owner: A,
            destination: ObjectDestination::Hand,
            program: Program::instant(
                ManaCost::free(),
                ExecutableBody::new(
                    BodyTarget::SingleCreature,
                    vec![Instruction::Damage { amount: 1.into() }],
                ),
            ),
        });
        cards.push(SeedCard {
            card: CardId(100),
            owner: A,
            destination: ObjectDestination::LibraryBottom,
            program: Program::creature(None, 2.into(), 4.into(), std::collections::BTreeSet::new()),
        });
        let game = GameState::initialize_scheduled(
            GameId(1510),
            GameContext::new(
                "r".into(),
                "qualification-synthetic".into(),
                "c".into(),
                "p".into(),
                "combat-fault".into(),
                crate::test_support::creature_types::synthetic_vocabulary().id(),
                spellwake_contracts::names::NameRules::literal_names().id(),
            )
            .unwrap(),
            {
                // Fixture premise: seeded BF timestamps follow this authored card sequence.
                let initial_battlefield_order = cards
                    .iter()
                    .filter(|card| {
                        matches!(
                            card.destination,
                            crate::object_identity::ObjectDestination::Battlefield { .. }
                        )
                    })
                    .map(|card| card.card)
                    .collect();
                GameSeed::with_uniform_life(
                    if multiplayer {
                        vec![B, C, A, D]
                    } else {
                        vec![A, B]
                    },
                    A,
                    cards,
                    initial_battlefield_order,
                    vec![],
                    20.into(),
                    CombatRules::UnrestrictedFreeForAll,
                    crate::test_support::creature_types::synthetic_vocabulary(),
                    std::sync::Arc::new(spellwake_contracts::names::NameRules::literal_names()),
                )
            },
            TurnStart::default(),
        )
        .unwrap()
        .into_state();
        let mut next = game;
        while next.turn().unwrap().position().step != Step::BeginningCombat {
            next = round(&next);
        }
        next
    }
    fn object(game: &GameState, id: u64) -> ObjectId {
        game.execution
            .window()
            .cards()
            .current_identity(CardId(id))
            .unwrap()
    }
    fn holder(game: &GameState) -> SeatId {
        let WindowPhase::Priority(PriorityPhase::Ready { holder }) =
            game.execution.window().phase()
        else {
            panic!("priority expected")
        };
        holder
    }
    fn command(game: &GameState, actor: SeatId, command: GameCommand) -> GameInput {
        GameInput {
            basis: game.basis.clone(),
            actor,
            command,
        }
    }
    fn pass(game: &GameState) -> GameState {
        game.advance(command(game, holder(game), GameCommand::Pass))
            .unwrap()
            .into_state()
    }
    fn round(game: &GameState) -> GameState {
        let mut next = game.clone();
        for _ in game.execution.window().participants() {
            next = pass(&next);
        }
        next
    }
    fn reply(game: &GameState, choice: DecisionReply) -> GameInput {
        let pending = game.pending.as_ref().unwrap();
        command(
            game,
            pending.chooser(),
            GameCommand::Reply {
                decision: pending.id(),
                choice,
            },
        )
    }
    fn attack(game: &GameState, selected: &[(u64, SeatId)]) -> GameState {
        game.advance(reply(
            game,
            DecisionReply::DeclareAttackers(
                selected
                    .iter()
                    .map(|&(id, defender)| AttackDeclaration {
                        attacker: object(game, id),
                        defender,
                    })
                    .collect(),
            ),
        ))
        .unwrap()
        .into_state()
    }
    fn block(game: &GameState, blocker: u64, attacker: u64) -> GameState {
        game.advance(reply(
            game,
            DecisionReply::DeclareBlockers(vec![BlockDeclaration {
                blocker: object(game, blocker),
                attacker: object(game, attacker),
            }]),
        ))
        .unwrap()
        .into_state()
    }

    fn pending_damage_assignments() -> GameState {
        let ready = round(&fixture(false));
        let attacking = attack(&ready, &[(1, B), (2, B)]);
        let blocking = round(&attacking);
        let blocked = blocking
            .advance(reply(
                &blocking,
                DecisionReply::DeclareBlockers(vec![
                    BlockDeclaration {
                        blocker: object(&blocking, 3),
                        attacker: object(&blocking, 1),
                    },
                    BlockDeclaration {
                        blocker: object(&blocking, 4),
                        attacker: object(&blocking, 1),
                    },
                ]),
            ))
            .unwrap()
            .into_state();
        let pending = round(&blocked);
        assert!(matches!(
            pending.pending.as_ref().unwrap().frame(),
            RuleFrame::Combat(CombatDecision::DamageAssignments(_))
        ));
        pending
    }

    fn whole_actor_assignments(game: &GameState) -> Vec<crate::combat::CombatDamageAssignment> {
        use crate::{combat::CombatDamageAssignment, damage::DamageRecipient};
        vec![
            CombatDamageAssignment {
                source: object(game, 1),
                target: DamageRecipient::Creature(object(game, 3)),
                amount: 1.into(),
            },
            CombatDamageAssignment {
                source: object(game, 1),
                target: DamageRecipient::Creature(object(game, 4)),
                amount: 1.into(),
            },
            CombatDamageAssignment {
                source: object(game, 2),
                target: DamageRecipient::Player(B),
                amount: 2.into(),
            },
        ]
    }

    #[test]
    fn damage_assignment_reply_covers_forced_sources_and_commits_one_simultaneous_batch() {
        use crate::combat::{AssignmentOrigin, CombatError};
        let game = pending_damage_assignments();
        let saved = game.clone();
        let mut missing_forced = whole_actor_assignments(&game);
        missing_forced.pop();
        assert_eq!(
            game.advance(reply(
                &game,
                DecisionReply::AssignCombatDamage(missing_forced)
            )),
            Err(GameError::Effect(EffectError::Combat(
                CombatError::IncorrectDamageTotal
            )))
        );
        assert_eq!(game, saved);
        assert_eq!(game.execution.life_total(B), Some(&20.into()));
        assert!(game.execution.pending_damage().is_none());
        for id in [1, 2, 3, 4] {
            assert_eq!(
                game.execution
                    .creature(object(&game, id))
                    .unwrap()
                    .unwrap()
                    .damage,
                0.into()
            );
        }
        assert_eq!(
            game.advance(command(&game, A, GameCommand::Pass)),
            Err(GameError::TurnWorkRequired(TurnWork::CombatDamage {
                additional: false
            }))
        );
        let completed = game
            .advance(reply(
                &game,
                DecisionReply::AssignCombatDamage(whole_actor_assignments(&game)),
            ))
            .unwrap();
        assert_eq!(game, saved);
        assert_eq!(completed.damage_journal().batches.len(), 1);
        assert_eq!(completed.damage_journal().attempts.len(), 5);
        assert_eq!(completed.damage_journal().applied.len(), 5);
        assert_eq!(completed.combat_assignments().len(), 2);
        assert_eq!(completed.combat_assignments()[0].player, A);
        assert_eq!(
            completed.combat_assignments()[0].origin,
            AssignmentOrigin::PlayerReply
        );
        assert_eq!(completed.combat_assignments()[0].assignments.len(), 3);
        assert_eq!(completed.combat_assignments()[1].player, B);
        assert_eq!(
            completed.combat_assignments()[1].origin,
            AssignmentOrigin::Forced
        );
        let state = completed.state();
        assert_eq!(state.execution.life_total(B), Some(&18.into()));
        assert!(
            state
                .execution
                .creature(object(&game, 1))
                .unwrap()
                .is_none()
        );
        for id in [3, 4] {
            assert_eq!(
                state
                    .execution
                    .creature(object(state, id))
                    .unwrap()
                    .unwrap()
                    .damage,
                1.into()
            );
        }
        assert_eq!(state.turn().unwrap().position().step, Step::CombatDamage);
        assert_eq!(state.turn().unwrap().stage(), TurnStage::Priority);
        assert_eq!(holder(state), A);
        let passed = pass(state);
        assert_eq!(passed.turn().unwrap().position().step, Step::CombatDamage);
    }

    #[test]
    fn late_damage_schedule_or_check_failure_preserves_the_entire_pending_actor() {
        for revision in [u64::MAX, u64::MAX - 1] {
            let mut game = pending_damage_assignments();
            game.execution.set_test_window_revision(revision);
            let saved = game.clone();
            assert!(
                game.advance(reply(
                    &game,
                    DecisionReply::AssignCombatDamage(whole_actor_assignments(&game)),
                ))
                .is_err()
            );
            assert_eq!(game, saved);
            assert_eq!(game.execution.life_total(B), Some(&20.into()));
            assert!(game.execution.pending_damage().is_none());
            assert!(game.pending.is_some());
        }
    }

    #[test]
    fn late_attack_card_schedule_and_check_failures_preserve_pending_choice_and_all_cards() {
        for fault in 0..3 {
            let mut game = round(&fixture(false));
            match fault {
                0 => game
                    .execution
                    .set_test_object_allocator(u64::MAX - 1, false),
                1 => game.execution.set_test_window_revision(u64::MAX - 1),
                _ => game.execution.set_test_window_revision(u64::MAX - 2),
            }
            let incoming = reply(
                &game,
                DecisionReply::DeclareAttackers(vec![
                    AttackDeclaration {
                        attacker: object(&game, 1),
                        defender: B,
                    },
                    AttackDeclaration {
                        attacker: object(&game, 2),
                        defender: B,
                    },
                ]),
            );
            let saved = game.clone();
            assert!(game.advance(incoming).is_err(), "fault {fault}");
            assert_eq!(game, saved);
            assert!(game.pending.is_some());
            assert!(game.execution.combat().is_none());
            for id in [1, 2] {
                assert_eq!(
                    game.execution
                        .window()
                        .cards()
                        .get(object(&game, id))
                        .unwrap()
                        .tapped(),
                    Some(false)
                );
            }
        }
    }

    #[test]
    fn failure_to_bind_the_new_attacker_decision_preserves_the_last_priority_pass() {
        for token in [false, true] {
            let mut game = pass(&fixture(false));
            if token {
                game.next_decision = u64::MAX;
            } else {
                game.execution.set_test_revision(u64::MAX - 1);
            }
            let saved = game.clone();
            assert!(game.advance(command(&game, B, GameCommand::Pass)).is_err());
            assert_eq!(game, saved);
            assert_eq!(game.turn().unwrap().position().step, Step::BeginningCombat);
            assert_eq!(holder(&game), B);
            assert!(game.pending.is_none());
            assert!(game.execution.combat().is_none());
        }
    }

    #[test]
    fn later_blocker_cursor_allocation_failure_keeps_earlier_accepted_defender() {
        let ready = round(&fixture(true));
        let attacking = attack(&ready, &[(1, D), (2, B), (7, C)]);
        let first = round(&attacking);
        assert_eq!(first.pending.as_ref().unwrap().chooser(), D);
        let mut next = block(&first, 6, 1);
        assert_eq!(next.pending.as_ref().unwrap().chooser(), B);
        next.next_decision = u64::MAX;
        let saved = next.clone();
        assert_eq!(
            next.advance(reply(
                &next,
                DecisionReply::DeclareBlockers(vec![BlockDeclaration {
                    blocker: object(&next, 3),
                    attacker: object(&next, 2)
                }])
            )),
            Err(GameError::DecisionIdentityExhausted)
        );
        assert_eq!(next, saved);
        let combat = next.execution.combat().unwrap();
        assert!(combat.blocker(object(&next, 6)).is_some());
        assert!(combat.blocker(object(&next, 3)).is_none());
        assert_eq!(
            combat.attacker(object(&next, 1)).unwrap().block_status(),
            BlockStatus::Blocked
        );
        assert_eq!(
            combat.attacker(object(&next, 2)).unwrap().block_status(),
            BlockStatus::NotDeclared
        );
    }

    fn nonempty_end_combat() -> GameState {
        let ready = round(&fixture(false));
        let attacking = attack(&ready, &[(1, B)]);
        let blocking = round(&attacking);
        let blocked = block(&blocking, 3, 1);
        let damage = round(&blocked);
        assert_eq!(damage.turn().unwrap().position().step, Step::CombatDamage);
        assert_eq!(damage.turn().unwrap().stage(), TurnStage::Priority);
        assert_eq!(
            damage
                .execution
                .creature(object(&damage, 1))
                .unwrap()
                .unwrap()
                .damage,
            2.into()
        );
        assert_eq!(
            damage
                .execution
                .creature(object(&damage, 3))
                .unwrap()
                .unwrap()
                .damage,
            2.into()
        );
        let end = round(&damage);
        assert_eq!(end.turn().unwrap().position().step, Step::EndCombat);
        end
    }

    #[test]
    fn private_nonempty_context_clears_only_after_actual_end_combat_priority_exit() {
        let initial = nonempty_end_combat();
        let combat = initial.execution.combat().unwrap().clone();
        assert_eq!(combat.attackers().count(), 1);
        assert_eq!(combat.blockers().count(), 1);
        let cast = initial
            .advance(command(
                &initial,
                A,
                GameCommand::Cast {
                    spell: object(&initial, 30),
                    target: TargetRef::Object(object(&initial, 4)),
                },
            ))
            .unwrap()
            .into_state();
        let resolved = round(&cast);
        assert_eq!(resolved.turn().unwrap().position().step, Step::EndCombat);
        assert_eq!(resolved.execution.combat(), Some(&combat));
        let before = resolved.execution.window().cards().clone();
        let first_pass = pass(&resolved);
        assert_eq!(first_pass.execution.combat(), Some(&combat));
        let exited = pass(&first_pass);
        assert_eq!(exited.turn().unwrap().position().step, Step::PostcombatMain);
        assert!(exited.execution.combat().is_none());
        assert_eq!(exited.execution.window().cards(), &before);
        assert_eq!(
            exited
                .execution
                .window()
                .cards()
                .get(object(&exited, 1))
                .unwrap()
                .tapped(),
            Some(true)
        );
    }

    #[test]
    fn private_nonempty_exit_failure_does_not_publish_metadata_clearing() {
        let mut game = pass(&nonempty_end_combat());
        game.execution.set_test_revision(u64::MAX - 1);
        let saved = game.clone();
        assert!(game.advance(command(&game, B, GameCommand::Pass)).is_err());
        assert_eq!(game, saved);
        assert_eq!(game.execution.combat().unwrap().blockers().count(), 1);
        assert_eq!(game.turn().unwrap().position().step, Step::EndCombat);
    }
}

#[cfg(test)]
mod targeted_trigger_tests;

#[cfg(test)]
mod reference_trace;
#[cfg(test)]
mod targeted_reference_tests;

#[cfg(test)]
mod conditional_reference_tests;

#[cfg(test)]
mod conditional_history_tests;
