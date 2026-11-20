//! One borrowed native applicability boundary for the predecessor capture epoch.
//! This visits stored values only: it never evaluates or executes a Program.
use super::projection::{EffectCall, QualificationCall, StackWindowCall};
use super::{Append, SnapshotOwner};
use spellwake_engine::{
    activated::{AbilityActivationRecord, AbilityActivationResult},
    characteristics::{PrintedText, ResolvedContinuousEffect, ResolvedPart},
    effects::{EffectDecision, EffectWindow, PendingWorkKind, RetainedStackPayload, RuleResult},
    game::{GameCommand, GameInput, GameState, NativeCandidate, RuleFrame},
    object_identity::{
        ControllerChange, GameObject, GameObjects, ObjectChange, ObjectMovement, ObjectOrigin,
        ObjectStatusChange, ZoneMoveOutcome, ZoneMoveResult,
    },
    occurrence::{
        EventCause, ExecutionBindings, RulesJournal, RulesOccurrence, RulesRecord, RulesTransition,
        SupportedAppearance, TriggerBinding, TriggerCauseSet, TriggerRelated, WaitingInstance,
        ZoneReason,
    },
    program::{ExecutableBody, Instruction, Program, ProgramKind, TriggeredAbility},
    setup::GameSeed,
    stack_window::{StackKind, StackWindow},
};
use std::fmt;

/// Typed exclusion from Current10; this is not a native game error.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ApplicabilityError;
impl fmt::Display for ApplicabilityError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("current10 does not admit resolution-selection semantics")
    }
}
impl std::error::Error for ApplicabilityError {}

// Crate visibility also permits the engine's cfg(test) owning descendant to
// inspect its private OrderedTriggerReceipt without adding a production getter.
pub(crate) mod sealed {
    pub trait Sealed {}
}

/// A maintained native result family whose retained semantics can be inspected.
/// Implementations are sealed; callers cannot replace inspection with a boolean.
///
/// ```compile_fail
/// use spellwake_reference_tools::current_profile::applicability::{ApplicabilityError, NativeCaptureValue};
/// #[derive(PartialEq, Eq)]
/// struct Unchecked;
/// impl NativeCaptureValue for Unchecked {
///     fn check_current10(&self) -> Result<(), ApplicabilityError> { Ok(()) }
/// }
/// ```
pub trait NativeCaptureValue: sealed::Sealed {
    /// Check stored native values for the predecessor epoch, without execution.
    ///
    /// # Errors
    /// Returns `ApplicabilityError` for represented resolution-selection or library-action semantics.
    fn check_current10(&self) -> Result<(), ApplicabilityError>;
}

macro_rules! native {
    ($ty:ty, $value:ident, $body:block) => {
        impl sealed::Sealed for $ty {}
        impl NativeCaptureValue for $ty {
            fn check_current10(&$value) -> Result<(), ApplicabilityError> $body
        }
    };
}
// These exact scalar/receipt families contain no Program, frame or occurrence.
// There is deliberately no blanket inert implementation for arbitrary Eq types.
macro_rules! inert {
    ($($ty:ty),* $(,)?) => { $(native!($ty, self, { Ok(()) });)* };
}
inert!(
    (),
    bool,
    u64,
    usize,
    String,
    serde_json::Value,
    spellwake_contracts::RulesInteger,
    spellwake_contracts::creature_types::CreatureTypeId,
    spellwake_engine::turn::TurnScheduler,
    spellwake_engine::effects::CreatureTypeChoice,
    spellwake_engine::shuffle::EntropyTape,
    spellwake_engine::shuffle::ShuffleRecord,
    spellwake_engine::mana::ManaPaymentRecord,
    spellwake_engine::mana::ManaProductionRecord,
    spellwake_engine::effects::ManaActivationResult
);

impl<T: NativeCaptureValue> sealed::Sealed for Option<T> {}
impl<T: NativeCaptureValue> NativeCaptureValue for Option<T> {
    fn check_current10(&self) -> Result<(), ApplicabilityError> {
        self.as_ref()
            .map_or(Ok(()), NativeCaptureValue::check_current10)
    }
}
impl<T: NativeCaptureValue> sealed::Sealed for Vec<T> {}
impl<T: NativeCaptureValue> NativeCaptureValue for Vec<T> {
    fn check_current10(&self) -> Result<(), ApplicabilityError> {
        self.iter()
            .try_for_each(NativeCaptureValue::check_current10)
    }
}
impl<A: NativeCaptureValue, B: NativeCaptureValue> sealed::Sealed for (A, B) {}
impl<A: NativeCaptureValue, B: NativeCaptureValue> NativeCaptureValue for (A, B) {
    fn check_current10(&self) -> Result<(), ApplicabilityError> {
        self.0.check_current10()?;
        self.1.check_current10()
    }
}
impl<S: NativeCaptureValue, R: NativeCaptureValue> sealed::Sealed for RulesTransition<S, R> {}
impl<S: NativeCaptureValue, R: NativeCaptureValue> NativeCaptureValue for RulesTransition<S, R> {
    fn check_current10(&self) -> Result<(), ApplicabilityError> {
        self.state().check_current10()?;
        self.result().check_current10()?;
        self.append().check_current10()
    }
}

native!(ExecutableBody, self, {
    self.instructions.iter().try_for_each(|value| match value {
        Instruction::Scry { .. }
        | Instruction::Surveil { .. }
        | Instruction::Mill { .. }
        | Instruction::Sacrifice { .. }
        | Instruction::Discard { .. } => Err(ApplicabilityError),
        Instruction::CreateTokens { creations } => creations
            .iter()
            .try_for_each(|creation| creation.definition.program().check_current10()),
        Instruction::BranchIfFalse { .. }
        | Instruction::Jump { .. }
        | Instruction::OptionalEffect { .. }
        | Instruction::OfferManaPayment { .. }
        | Instruction::AdjustControllerLife { .. }
        | Instruction::AdjustTargetLife { .. }
        | Instruction::ChooseCreatureType { .. }
        | Instruction::Damage { .. }
        | Instruction::CreatePreventionShield { .. }
        | Instruction::CreateDamageMultiplier { .. }
        | Instruction::ReturnToOwnersHand
        | Instruction::CounterSpell
        | Instruction::CopySpell
        | Instruction::CopyTargetToSource { .. }
        | Instruction::CreateContinuousEffect { .. }
        | Instruction::PutPowerToughnessCounters { .. }
        | Instruction::DrawController { .. } => Ok(()),
    })
});
native!(TriggeredAbility, self, {
    if matches!(
        self.condition,
        spellwake_engine::program::OrdinaryCondition::Scried(_)
            | spellwake_engine::program::OrdinaryCondition::Surveilled(_)
    ) {
        return Err(ApplicabilityError);
    }
    self.body.check_current10()
});
fn abilities(
    activated: &[spellwake_engine::activated::ActivatedAbility],
    triggered: &[TriggeredAbility],
) -> Result<(), ApplicabilityError> {
    activated
        .iter()
        .try_for_each(|value| value.body.check_current10())?;
    triggered
        .iter()
        .try_for_each(NativeCaptureValue::check_current10)
}
native!(Program, self, {
    if !self.zone_replacements.is_empty() {
        return Err(ApplicabilityError);
    }
    match &self.kind {
        ProgramKind::Spell(spell) => spell.body.check_current10()?,
        ProgramKind::Permanent(_) => {}
    }
    abilities(&self.activated, &self.triggered)
});
native!(PrintedText, self, {
    if !self.zone_replacements.is_empty() {
        return Err(ApplicabilityError);
    }
    abilities(&self.activated, &self.triggered)
});
native!(GameSeed, self, {
    self.cards
        .iter()
        .try_for_each(|value| value.program.check_current10())
});
native!(GameObject, self, {
    match self.origin() {
        ObjectOrigin::Card(_) => Ok(()),
        ObjectOrigin::Token { definition } => definition.check_current10(),
    }
});
native!(GameObjects, self, {
    self.iter()
        .try_for_each(NativeCaptureValue::check_current10)
});
native!(StackKind, self, {
    match self {
        StackKind::ActivatedAbility { source } | StackKind::TriggeredAbility { source } => {
            source.check_current10()
        }
        StackKind::CardSpell(_) | StackKind::SpellCopy { .. } => Ok(()),
    }
});
native!(StackWindow, self, {
    self.cards().check_current10()?;
    self.entries()
        .iter()
        .chain(self.resolving())
        .chain(self.checking_targets())
        .try_for_each(|entry| entry.kind().check_current10())
});
native!(ResolvedContinuousEffect, self, {
    self.parts.0.iter().try_for_each(|value| match value {
        ResolvedPart::Copy(program) => program.check_current10(),
        ResolvedPart::Control(_)
        | ResolvedPart::Text(_)
        | ResolvedPart::Names(_)
        | ResolvedPart::Type(_)
        | ResolvedPart::Color(_)
        | ResolvedPart::Abilities(_)
        | ResolvedPart::SetPT(_)
        | ResolvedPart::ModifyPT(_)
        | ResolvedPart::SwitchPT => Ok(()),
    })
});
native!(SupportedAppearance, self, {
    if !self.abilities.zone_replacements.is_empty() {
        return Err(ApplicabilityError);
    }
    self.object.check_current10()?;
    self.printed_text.check_current10()
});
native!(TriggerCauseSet, self, {
    self.members()
        .iter()
        .try_for_each(|member| match &member.related {
            TriggerRelated::LeftGame { .. } => Err(ApplicabilityError),
            TriggerRelated::LibraryAction(_) => Err(ApplicabilityError),
            TriggerRelated::SpellCast(value) => value.spell().check_current10(),
            TriggerRelated::TokenCreated { after, .. } => after.check_current10(),
            TriggerRelated::ZoneChange { before, after }
            | TriggerRelated::Discard { before, after, .. } => {
                before.check_current10()?;
                after.check_current10()
            }
            TriggerRelated::StatusChange(value) => value.check_current10(),
            TriggerRelated::LifeChanged(_) | TriggerRelated::Step { .. } => Ok(()),
        })
});
native!(WaitingInstance, self, {
    self.body.check_current10()?;
    self.source.check_current10()?;
    self.causes.check_current10()
});
native!(TriggerBinding, self, {
    self.source.check_current10()?;
    self.causes.check_current10()
});
native!(ExecutionBindings, self, { self.trigger.check_current10() });
native!(RetainedStackPayload<'_>, self, {
    if let Some(body) = self.body {
        body.check_current10()?;
    }
    if let Some(triggered) = self.copied_triggered {
        triggered
            .iter()
            .try_for_each(NativeCaptureValue::check_current10)?;
    }
    if let Some(bindings) = self.bindings {
        bindings.check_current10()?;
    }
    Ok(())
});
native!(EffectWindow, self, {
    if matches!(
        self.resolution_choice_state(),
        Some(spellwake_engine::effects::ResolutionChoiceState::ChooserSubstitution { .. })
    ) {
        return Err(ApplicabilityError);
    }
    self.programs()
        .try_for_each(|(_, value)| value.check_current10())?;
    self.window().check_current10()?;
    if let Some(batch) = self.pending_damage() {
        for component in batch.components() {
            component.attempt().source.check_current10()?;
        }
    }
    self.continuous_effects()
        .try_for_each(NativeCaptureValue::check_current10)?;
    self.waiting_triggers()
        .try_for_each(NativeCaptureValue::check_current10)?;
    self.retained_stack_payloads()
        .try_for_each(|payload| payload.check_current10())?;
    if self.has_check_replacement_work() {
        return Err(ApplicabilityError);
    }
    match self.pending_work_kind() {
        Some(PendingWorkKind::LibraryTop | PendingWorkKind::ResolutionSelection) => {
            Err(ApplicabilityError)
        }
        Some(
            PendingWorkKind::Resolution
            | PendingWorkKind::OptionalEffect
            | PendingWorkKind::ResolutionPayment
            | PendingWorkKind::ManaColor
            | PendingWorkKind::TokenEntry
            | PendingWorkKind::Damage
            | PendingWorkKind::Checks
            | PendingWorkKind::Combat,
        )
        | None => Ok(()),
    }
});
native!(RuleFrame, self, {
    match self {
        RuleFrame::ReplacementChooser(_) => Err(ApplicabilityError),
        RuleFrame::Checks(spellwake_engine::checks::CheckDecision::CleanupSelect(_)) => {
            Err(ApplicabilityError)
        }
        RuleFrame::ZoneReplacement(_) => Err(ApplicabilityError),
        RuleFrame::LibraryTop(_) => Err(ApplicabilityError),
        RuleFrame::ResolutionSelect(_) | RuleFrame::ResolutionGraveyardOrder(_) => {
            Err(ApplicabilityError)
        }
        RuleFrame::TimestampOrder(value) => value
            .members()
            .iter()
            .try_for_each(|member| member.definition.check_current10()),
        RuleFrame::Damage(value) => value.check_current10(),
        RuleFrame::OptionalEffect(_)
        | RuleFrame::ResolutionPayment(_)
        | RuleFrame::Checks(_)
        | RuleFrame::Triggers(_)
        | RuleFrame::TriggerTarget(_)
        | RuleFrame::Opening(_)
        | RuleFrame::CopyTargets(_)
        | RuleFrame::ManaColor(_)
        | RuleFrame::Combat(_)
        | RuleFrame::CreatureType(_) => Ok(()),
    }
});
native!(EffectDecision, self, {
    match self {
        EffectDecision::ReplacementChooser(_) => Err(ApplicabilityError),
        EffectDecision::Checks(spellwake_engine::checks::CheckDecision::CleanupSelect(_)) => {
            Err(ApplicabilityError)
        }
        EffectDecision::ZoneReplacement(_) => Err(ApplicabilityError),
        EffectDecision::LibraryTop(_) => Err(ApplicabilityError),
        EffectDecision::ResolutionSelect(_) | EffectDecision::ResolutionGraveyardOrder(_) => {
            Err(ApplicabilityError)
        }
        EffectDecision::TimestampOrder(value) => value
            .members()
            .iter()
            .try_for_each(|member| member.definition.check_current10()),
        EffectDecision::Damage(value) => value.check_current10(),
        EffectDecision::OptionalEffect(_)
        | EffectDecision::ResolutionPayment(_)
        | EffectDecision::Checks(_)
        | EffectDecision::Triggers(_)
        | EffectDecision::TriggerTarget(_)
        | EffectDecision::CopyTargets(_)
        | EffectDecision::Combat(_)
        | EffectDecision::CreatureType(_) => Ok(()),
    }
});
native!(GameState, self, {
    if self.conclusion().is_some_and(|c| {
        matches!(
            c.cause(),
            spellwake_engine::game::GameConclusionCause::Conceded { .. }
                | spellwake_engine::game::GameConclusionCause::ConcededLastSurvivor { .. }
                | spellwake_engine::game::GameConclusionCause::BankExpired { .. }
                | spellwake_engine::game::GameConclusionCause::ResponseExpiredLastSurvivor { .. }
        )
    }) {
        return Err(ApplicabilityError);
    }
    self.execution().check_current10()?;
    if let Some(decision) = self.pending_decision() {
        decision.frame().check_current10()?;
    }
    Ok(())
});
native!(NativeCandidate, self, {
    self.state().check_current10()?;
    self.rules_journal().check_current10()?;
    self.draw_journal().check_current10()?;
    self.check_journal().check_current10()?;
    self.damage_journal().check_current10()?;
    self.combat_declarations()
        .iter()
        .try_for_each(NativeCaptureValue::check_current10)?;
    self.permanent_entry()
        .map_or(Ok(()), NativeCaptureValue::check_current10)?;
    self.land_play()
        .map_or(Ok(()), NativeCaptureValue::check_current10)?;
    self.ability_activation()
        .map_or(Ok(()), NativeCaptureValue::check_current10)
});
native!(RuleResult, self, {
    match self {
        RuleResult::Complete {
            checks,
            draws,
            journal,
            permanent_entry,
            declarations,
            ..
        } => {
            declarations.check_current10()?;
            checks.check_current10()?;
            journal.check_current10()?;
            draws.check_current10()?;
            permanent_entry.check_current10()
        }
        RuleResult::Pending {
            checks,
            draws,
            journal,
            frame,
            declarations,
            ..
        } => {
            declarations.check_current10()?;
            checks.check_current10()?;
            journal.check_current10()?;
            draws.check_current10()?;
            frame.check_current10()
        }
    }
});
native!(spellwake_engine::effects::DecisionReply, self, {
    use spellwake_engine::effects::DecisionReply;
    match self {
        DecisionReply::ReplacementChooser(_) => Err(ApplicabilityError),
        DecisionReply::ZoneReplacement(_) => Err(ApplicabilityError),
        DecisionReply::LibraryTop(_) => Err(ApplicabilityError),
        DecisionReply::SelectResolution(_) | DecisionReply::OrderResolutionGraveyard(_) => {
            Err(ApplicabilityError)
        }
        DecisionReply::OptionalEffect(_)
        | DecisionReply::ResolutionPayment(_)
        | DecisionReply::TimestampOrder(_)
        | DecisionReply::Discard(_)
        | DecisionReply::OrderGraveyard(_)
        | DecisionReply::KeepLegend { .. }
        | DecisionReply::OrderTriggers(_)
        | DecisionReply::TriggerTarget(_)
        | DecisionReply::DeclareAttackers(_)
        | DecisionReply::DeclareBlockers(_)
        | DecisionReply::AssignCombatDamage(_)
        | DecisionReply::Mulligan(_)
        | DecisionReply::Bottom(_)
        | DecisionReply::CopyTargets(_)
        | DecisionReply::DamageOrder(_)
        | DecisionReply::AllocatePrevention(_)
        | DecisionReply::ManaColor(_)
        | DecisionReply::CreatureType(_) => Ok(()),
    }
});
native!(GameInput, self, {
    match &self.command {
        GameCommand::Concede => Err(ApplicabilityError),
        GameCommand::Reply { choice, .. } => choice.check_current10(),
        GameCommand::CastUntargeted { .. }
        | GameCommand::PlayLand { .. }
        | GameCommand::ActivateAbility { .. }
        | GameCommand::ActivateMana { .. }
        | GameCommand::Cast { .. }
        | GameCommand::CastPaid { .. }
        | GameCommand::Pass => Ok(()),
    }
});

/// Inspect the exact call operands before either Result is projected.
pub(super) fn call(value: &QualificationCall<'_>) -> Result<(), ApplicabilityError> {
    match value {
        QualificationCall::GameInput(value) => value.check_current10(),
        QualificationCall::GameInitialize { seed, .. }
        | QualificationCall::GameInitializeScheduled { seed, .. }
        | QualificationCall::EffectInitialize(seed) => seed.check_current10(),
        QualificationCall::GameInitializeOpening { setup, .. } => setup.seed.check_current10(),
        QualificationCall::ProgramValidate { program, .. } => program.check_current10(),
        QualificationCall::EffectCall(value) => match value {
            EffectCall::ResumeRule { frame, reply, .. } => {
                frame.check_current10()?;
                reply.check_current10()
            }
            EffectCall::BeginDamageBatch {
                inputs,
                damage,
                draws,
                declarations,
                ..
            } => {
                declarations
                    .iter()
                    .try_for_each(NativeCaptureValue::check_current10)?;
                inputs
                    .iter()
                    .try_for_each(|value| value.source.check_current10())?;
                damage.check_current10()?;
                draws.check_current10()
            }
            EffectCall::BeginOpening { .. }
            | EffectCall::OpeningShuffle { .. }
            | EffectCall::QualificationDiscard { .. }
            | EffectCall::QualificationSetTapped { .. }
            | EffectCall::RecordTurnStart { .. }
            | EffectCall::UntapOnTurn { .. }
            | EffectCall::CastOnTurn { .. }
            | EffectCall::ActivateAbility { .. }
            | EffectCall::QualificationZoneMoves { .. }
            | EffectCall::QualificationControllerSuccessor { .. }
            | EffectCall::BeginCleanup { .. }
            | EffectCall::EnterCalendar { .. }
            | EffectCall::BeginManaActivation { .. }
            | EffectCall::AcceptCreatureType { .. }
            | EffectCall::QualificationBeginSharedDamage { .. }
            | EffectCall::QualificationPrepareResolutionCursor { .. }
            | EffectCall::QualificationPrepareCreatureFields { .. }
            | EffectCall::QualificationPrepareContextFields { .. }
            | EffectCall::CreatureTypeChoice
            | EffectCall::QualificationPrepareEmptyCombat
            | EffectCall::QualificationPruneCreatureState
            | EffectCall::Pass { .. }
            | EffectCall::BeginResolution { .. }
            | EffectCall::BeginChecks { .. }
            | EffectCall::SettleChecks { .. }
            | EffectCall::ResolveTop { .. }
            | EffectCall::Cast { .. }
            | EffectCall::CastWithPayment { .. }
            | EffectCall::ActivateMana { .. }
            | EffectCall::DrawCards { .. }
            | EffectCall::EnterTurnStep { .. } => Ok(()),
        },
        QualificationCall::StackWindowCall(value) => match value {
            StackWindowCall::RecordOrderedTriggersWithReceipt { triggers, .. } => triggers
                .iter()
                .try_for_each(|trigger| trigger.source.check_current10()),
            StackWindowCall::RecordCardSpell { .. }
            | StackWindowCall::RecordResolutionMove { .. }
            | StackWindowCall::FinishChecks { .. }
            | StackWindowCall::Pass { .. }
            | StackWindowCall::CheckResolutionTargets { .. }
            | StackWindowCall::FinishResolution { .. }
            | StackWindowCall::RecordTapAction { .. } => Ok(()),
        },
        QualificationCall::GameRelatedPlayerCauseFault { .. }
        | QualificationCall::GameSupplyEntropy { .. }
        | QualificationCall::GameObjectsInitialize { .. }
        | QualificationCall::GameObjectsCall(_)
        | QualificationCall::TurnCall(_)
        | QualificationCall::PureCall { .. } => Ok(()),
    }
}
pub(super) fn snapshot(value: &SnapshotOwner<'_>) -> Result<(), ApplicabilityError> {
    match value {
        SnapshotOwner::GameState(value) => value.check_current10(),
        SnapshotOwner::EffectWindow(value) => value.check_current10(),
        SnapshotOwner::StackWindow(value) => value.check_current10(),
        SnapshotOwner::GameObjects { cards, .. } => cards.check_current10(),
        SnapshotOwner::InitializeInput { seed, .. } => seed.check_current10(),
        SnapshotOwner::ProgramInput { program, .. } => program.check_current10(),
        SnapshotOwner::TurnScheduler(_) | SnapshotOwner::PureValue(_) => Ok(()),
    }
}
pub(super) fn append(value: &Append<'_>) -> Result<(), ApplicabilityError> {
    match value {
        Append::RulesJournal(value) => value.check_current10(),
        Append::NotApplicable => Ok(()),
    }
}
native!(RulesRecord, self, {
    match self {
        RulesRecord::ChooserReplaced { .. } => return Err(ApplicabilityError),
        RulesRecord::GameConcluded { .. } | RulesRecord::PlayersDeparted { .. } => {
            return Err(ApplicabilityError);
        }
        RulesRecord::Event(event) => {
            for occurrence in &event.occurrences {
                if matches!(event.cause, EventCause::Instruction { .. })
                    && matches!(&occurrence.value, RulesOccurrence::ZoneChange(value)
                            if matches!(value.reason, ZoneReason::Discard { .. }))
                {
                    return Err(ApplicabilityError);
                }
                occurrence.value.check_current10()?;
            }
        }
        RulesRecord::TriggerDiscovered(value) => value.check_current10()?,
        RulesRecord::OptionalEffectOutcome { .. }
        | RulesRecord::ResolutionPaymentOutcome { .. }
        | RulesRecord::InstructionBranch { .. }
        | RulesRecord::InstructionJump { .. }
        | RulesRecord::TriggerPlacementStarted(_)
        | RulesRecord::TriggerTargetSelected { .. }
        | RulesRecord::TriggerRemoved { .. }
        | RulesRecord::TriggerPlaced(_) => {}
    }
    Ok(())
});
native!(RulesJournal, self, {
    self.records
        .iter()
        .try_for_each(NativeCaptureValue::check_current10)
});
native!(RulesOccurrence, self, {
    match self {
        RulesOccurrence::ObjectLeftGame { .. } => Err(ApplicabilityError),
        RulesOccurrence::LibraryAction(_) => Err(ApplicabilityError),
        RulesOccurrence::TokenCreated { after, .. } => after.check_current10(),
        RulesOccurrence::TokenCeased { object } => object.check_current10(),
        RulesOccurrence::ControllerChanged(value) => value.check_current10(),
        RulesOccurrence::ZoneChange(value) => {
            match value.reason {
                ZoneReason::PlayerDeparture
                | ZoneReason::DefaultControllerDeparted
                | ZoneReason::Surveil { .. }
                | ZoneReason::Mill { .. }
                | ZoneReason::Sacrifice { .. } => return Err(ApplicabilityError),
                ZoneReason::Opening(_)
                | ZoneReason::LandPlay
                | ZoneReason::CastAnnouncement
                | ZoneReason::PermanentResolution
                | ZoneReason::ResolutionExit
                | ZoneReason::AllTargetsIllegalExit
                | ZoneReason::ReturnToOwnersHand
                | ZoneReason::Countered
                | ZoneReason::SacrificeCost
                | ZoneReason::Draw { .. }
                | ZoneReason::Discard { .. }
                | ZoneReason::StateBased(_)
                | ZoneReason::MechanismUnit => {}
            }
            value.before.check_current10()?;
            value.after.check_current10()
        }
        RulesOccurrence::StatusChange { change, .. } => change.check_current10(),
        RulesOccurrence::CastCompleted(value) => value.spell().check_current10(),
        RulesOccurrence::ActivatedAbility { source, .. }
        | RulesOccurrence::ManaAbility { source, .. } => source.check_current10(),
        RulesOccurrence::CopyCreated {
            copiable_triggered, ..
        } => copiable_triggered.check_current10(),
        RulesOccurrence::ContinuousEffectCreated {
            effect,
            appearances,
        } => {
            effect.check_current10()?;
            appearances.iter().try_for_each(|value| {
                value.before.check_current10()?;
                value.after.check_current10()
            })
        }
        RulesOccurrence::CleanupCharacteristics { appearances, .. } => {
            appearances.iter().try_for_each(|value| {
                value.before.check_current10()?;
                value.after.check_current10()
            })
        }
        RulesOccurrence::CountersPlaced {
            before_appearance,
            after_appearance,
            ..
        } => {
            before_appearance.check_current10()?;
            after_appearance.check_current10()
        }
        RulesOccurrence::EntryCompleted { kind, .. } => kind.check_current10(),
        RulesOccurrence::DamageDealt { damage, .. } => damage.source.check_current10(),
        RulesOccurrence::LifeChanged(_)
        | RulesOccurrence::CreatureTypeChosen { .. }
        | RulesOccurrence::Shuffled(_)
        | RulesOccurrence::FailedDraw { .. }
        | RulesOccurrence::ManaProduced { .. }
        | RulesOccurrence::ManaDebited { .. }
        | RulesOccurrence::ManaPoolEmptied { .. }
        | RulesOccurrence::WorldTenureChanged(_)
        | RulesOccurrence::CopyMoved { .. }
        | RulesOccurrence::CopyCeased { .. }
        | RulesOccurrence::CombatantsRemoved { .. }
        | RulesOccurrence::CountersCancelled { .. }
        | RulesOccurrence::DamageEffectCreated(_)
        | RulesOccurrence::DamagePrevented { .. }
        | RulesOccurrence::DamageMultiplied(_)
        | RulesOccurrence::DamageResult { .. }
        | RulesOccurrence::PlayerLost { .. }
        | RulesOccurrence::AttackersDeclared { .. }
        | RulesOccurrence::BlockersDeclared { .. }
        | RulesOccurrence::CombatDamageAssigned(_)
        | RulesOccurrence::CombatEnded { .. }
        | RulesOccurrence::StepBegan { .. } => Ok(()),
    }
});

macro_rules! before_after {
    ($($ty:ty),* $(,)?) => { $(native!($ty, self, {
        self.before.check_current10()?;
        self.after.check_current10()
    });)* };
}
before_after!(
    spellwake_engine::combat::AttackTap,
    ObjectMovement,
    ObjectStatusChange,
    ControllerChange,
    spellwake_engine::actions::PermanentEntryRecord,
    spellwake_engine::actions::LandPlayRecord
);
native!(ObjectChange, self, {
    self.next.check_current10()?;
    self.before.check_current10()?;
    self.after.check_current10()
});
native!(ZoneMoveOutcome, self, {
    match self {
        ZoneMoveOutcome::Moved { movement } => movement.check_current10(),
        ZoneMoveOutcome::Unchanged { object, .. } => object.check_current10(),
    }
});
native!(ZoneMoveResult, self, {
    self.next.check_current10()?;
    self.outcome.check_current10()
});
native!(AbilityActivationRecord, self, {
    self.source.check_current10()
});
native!(AbilityActivationResult, self, {
    self.activation.check_current10()
});
native!(spellwake_engine::draw::DrawJournal, self, {
    self.events.iter().try_for_each(|value| match value {
        spellwake_engine::draw::DrawEvent::Drawn { before, after, .. } => {
            before.check_current10()?;
            after.check_current10()
        }
        spellwake_engine::draw::DrawEvent::EmptyLibrary { .. } => Ok(()),
    })
});
native!(spellwake_engine::draw::LibraryDraw, self, {
    self.next.check_current10()?;
    self.journal.check_current10()
});
native!(spellwake_engine::shuffle::LibraryShuffle, self, {
    self.next.check_current10()
});

native!(spellwake_engine::combat::CombatDeclarationRecord, self, {
    match self {
        spellwake_engine::combat::CombatDeclarationRecord::Attackers { taps, .. } => {
            taps.check_current10()
        }
        spellwake_engine::combat::CombatDeclarationRecord::Blockers { .. } => Ok(()),
    }
});
native!(spellwake_engine::checks::CheckJournal, self, {
    if self.choices.iter().any(|record| {
        matches!(
            record.choice,
            spellwake_engine::checks::CheckChoice::CleanupSelected { .. }
        )
    }) {
        return Err(ApplicabilityError);
    }
    self.moves.iter().try_for_each(|value| {
        value.before.check_current10()?;
        value.after.check_current10()
    })
});
native!(spellwake_engine::damage::DamageSource, self, {
    match self {
        spellwake_engine::damage::DamageSource::Object { object, .. } => object.check_current10(),
        spellwake_engine::damage::DamageSource::SpellCopy { .. } => Ok(()),
    }
});
native!(spellwake_engine::damage::DamageJournal, self, {
    self.attempts
        .iter()
        .try_for_each(|value| value.source.check_current10())?;
    self.applied
        .iter()
        .try_for_each(|value| value.source.check_current10())
});
native!(spellwake_engine::effects::DamageDecision, self, {
    match self {
        spellwake_engine::effects::DamageDecision::Allocation(value) => value
            .eligible()
            .iter()
            .try_for_each(|value| value.source().check_current10()),
        spellwake_engine::effects::DamageDecision::Order(_) => Ok(()),
    }
});

#[cfg(test)]
#[path = "applicability/tests.rs"]
pub(crate) mod tests;
