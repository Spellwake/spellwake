//! One borrowed native applicability boundary for the predecessor capture epoch.
//! This visits stored values only: it never evaluates or executes a Program.
use super::projection::{EffectCall, QualificationCall, StackWindowCall};
use super::{Append, SnapshotOwner};
use spellwake_engine::{
    activated::{AbilityActivationRecord, AbilityActivationResult},
    characteristics::{
        AbilityChange, KeywordAbility, PrintedText, ResolvedContinuousEffect, ResolvedPart,
        ResolvedPartSpec, StaticContinuousAbility, StaticPart,
    },
    combat::CombatDecision,
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
    spellwake_engine::shuffle::ShuffleRecord
);
native!(spellwake_engine::effects::ManaActivationResult, self, {
    match self {
        spellwake_engine::effects::ManaActivationResult::Complete { production } => {
            production.check_current10()
        }
        spellwake_engine::effects::ManaActivationResult::Pending { .. } => Ok(()),
        spellwake_engine::effects::ManaActivationResult::PendingRule { .. } => {
            Err(ApplicabilityError)
        }
    }
});
native!(spellwake_engine::mana::ManaPaymentRecord, self, {
    if self.restricted_mana.is_empty() && self.from_lots.is_empty() {
        Ok(())
    } else {
        Err(ApplicabilityError)
    }
});
native!(spellwake_engine::mana::ManaProductionRecord, self, {
    if self.restriction.is_none() {
        Ok(())
    } else {
        Err(ApplicabilityError)
    }
});

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

fn historical_keywords(
    keywords: &std::collections::BTreeSet<KeywordAbility>,
) -> Result<(), ApplicabilityError> {
    if keywords
        .iter()
        .all(|keyword| matches!(keyword, KeywordAbility::Haste | KeywordAbility::Vigilance))
    {
        Ok(())
    } else {
        Err(ApplicabilityError)
    }
}
native!(AbilityChange, self, {
    if !self.granted_triggers.is_empty()
        || self.grants_unblockable
        || self.granted_toxic.is_some()
        || self.granted_exalted != 0
        || !self.granted_combat_keywords.is_empty()
    {
        return Err(ApplicabilityError);
    }
    historical_keywords(&self.granted_keywords)
});
native!(StaticContinuousAbility, self, {
    use spellwake_engine::characteristics::{
        CastPermissionRule, SpellCostModifier, StaticDamageRule, StaticDrawRule,
        StaticModifierRule, StaticRestriction, StaticUntapRule,
    };
    let rejected = match self {
        Self::Battlefield { .. }
        | Self::CharacteristicDefining { .. }
        | Self::CommandZoneActive { .. } => false,
        Self::CostModifier { modifier, .. } => matches!(
            modifier,
            SpellCostModifier::Discount { .. }
                | SpellCostModifier::IncreaseExceptType { .. }
                | SpellCostModifier::HighManaValueProtected { .. }
                | SpellCostModifier::PayLifeForEnchantmentOwnTurnOnce
                | SpellCostModifier::FreeColorlessHandSpellOnce
                | SpellCostModifier::NonartifactSpellsHaveImprovise
        ),
        Self::CastPermission { permission, .. } => matches!(
            permission,
            CastPermissionRule::Flash { .. }
                | CastPermissionRule::AdditionalLandPlays(_)
                | CastPermissionRule::ControllerSpellsCannotBeCountered(_)
        ),
        Self::DamageRule { rule, .. } => matches!(
            rule,
            StaticDamageRule::PreventEnchantedCreature { .. }
                | StaticDamageRule::DoubleToEnchantedPlayer
                | StaticDamageRule::PreventIncomingCombatAndExile
                | StaticDamageRule::ReplaceSubtypeCombatWithMill(_)
                | StaticDamageRule::PreventIncomingRemovePlusOne
                | StaticDamageRule::LethalUsesPower
                | StaticDamageRule::PreventSourceCombat
                | StaticDamageRule::PreventNoncombat
        ),
        Self::DrawRule { rule, .. } => matches!(
            rule,
            StaticDrawRule::SplitOpponentsMultipleDraw
                | StaticDrawRule::ReplaceWithLandKind
                | StaticDrawRule::EmptyDrawWins
                | StaticDrawRule::OpponentsAtMostOne
                | StaticDrawRule::NoMaximumHandSize
        ),
        Self::UntapRule { rule, .. } => matches!(
            rule,
            StaticUntapRule::EnchantedCreatureUntapsOnlyForMonarch
                | StaticUntapRule::EnchantedCreatureSkipsUntap
                | StaticUntapRule::SourceSkipsUntap
        ),
        Self::Restriction { rule, .. } => matches!(
            rule,
            StaticRestriction::PreventLoyaltyActivations
                | StaticRestriction::PreventArtifactActivations
                | StaticRestriction::PreventControlledTransform { .. }
                | StaticRestriction::ProhibitLifeAndCreatureCosts
                | StaticRestriction::ProhibitOpponentsCausingSacrifice
                | StaticRestriction::EnchantedPlayerCastLimit
                | StaticRestriction::ProhibitCastingChosenName(_)
                | StaticRestriction::RestrictGraveyardAndLibrary
                | StaticRestriction::RestrictOpponentsDuringOwnTurn
                | StaticRestriction::OpponentsCannotGainLife
        ),
        Self::Modifier { rule, .. } => matches!(
            rule,
            StaticModifierRule::AdditionalBlockWhileMonarch
                | StaticModifierRule::EnterControlledCreaturesAsEnchantedCopy
                | StaticModifierRule::EnterControlledCreaturesAsSourceCopy
                | StaticModifierRule::AdditionalCreatureDeathTrigger
                | StaticModifierRule::RetainMana(_)
                | StaticModifierRule::AdditionalSelfPlusOneCounter
                | StaticModifierRule::DoubleCreatedTokens
                | StaticModifierRule::ExtraEntryLoyalty
                | StaticModifierRule::TwiceLoyaltyPerTurn
        ),
        Self::EventReplacement { .. } => false,
    };
    if rejected {
        return Err(ApplicabilityError);
    }

    if let Self::Battlefield {
        selector, parts, ..
    } = self
    {
        if selector.attached_to_source || selector.token.is_some() || selector.any_counter {
            return Err(ApplicabilityError);
        }
        for part in &parts.0 {
            if matches!(
                part,
                StaticPart::InheritCounteredCreatureAbilities
                    | StaticPart::GrantActivated(_)
                    | StaticPart::GrantActivatedRestricted { .. }
                    | StaticPart::GrantTrigger(_)
                    | StaticPart::GrantFixedColorMana(_)
                    | StaticPart::BlockingRestriction(_)
                    | StaticPart::GrantRestriction(_)
            ) {
                return Err(ApplicabilityError);
            }
            if let StaticPart::Abilities(change) = part {
                change.check_current10()?;
            }
        }
    }
    Ok(())
});
native!(ExecutableBody, self, {
    let target_rejected = {
        use spellwake_engine::program::{
            AnyTargetKind, BodyTarget, ObjectFilter, SpellFilter, TargetZone,
        };
        match self.target {
            BodyTarget::None | BodyTarget::Player { .. } => false,
            BodyTarget::Object { zone, filter } => match zone {
                TargetZone::Battlefield => {
                    filter != ObjectFilter::any() && filter != ObjectFilter::creature()
                }
                TargetZone::OwnGraveyard => filter != ObjectFilter::any(),
                TargetZone::AnyGraveyard { .. } => true,
                TargetZone::Exile {
                    owned_flashback_only,
                } => !owned_flashback_only || filter != ObjectFilter::any(),
                TargetZone::RecentlyDiscardedOrFromLibrary => false,
            },
            BodyTarget::AnyTarget(kind) => matches!(
                kind,
                AnyTargetKind::Any
                    | AnyTargetKind::PlayerOrPlaneswalker
                    | AnyTargetKind::OpponentOrPlaneswalker
            ),
            BodyTarget::Spell {
                filter,
                include_abilities,
            } => {
                !include_abilities
                    && (filter == SpellFilter::creature() || filter == SpellFilter::noncreature())
            }
            BodyTarget::StackEntry(_) => true,
        }
    };
    if target_rejected {
        return Err(ApplicabilityError);
    }
    if self.triggered_modes().is_some() || self.permanent_target_bounds.is_some() {
        return Err(ApplicabilityError);
    }
    self.instructions.iter().try_for_each(|value| match value {
        Instruction::ForEach { .. }
        | Instruction::ForEachPlayerQuery { .. }
        | Instruction::BindEnv { .. }
        | Instruction::SetObjectTag { .. }
        | Instruction::UpdateStateTrack { .. }
        | Instruction::SideDeckOp(_)
        | Instruction::ScheduleDelayedEventTrigger { .. }
        | Instruction::FixedTargetClause(_)
        | Instruction::SearchLibrary { .. }
        | Instruction::ScheduleDelayedStepTrigger { .. }
        | Instruction::EmitReflexiveTrigger { .. } => Err(ApplicabilityError),
        Instruction::PromptChoice(spec) if spec.as_mana_payment().is_some() => {
            Err(ApplicabilityError)
        }
        Instruction::PromptChoice(spec) => {
            if matches!(
                &spec.domain,
                spellwake_engine::toolbox::PromptDomain::Subtype {
                    kind: spellwake_engine::toolbox::SubtypeChoiceKind::Creature,
                    legacy_slot: Some(_),
                    ..
                }
            ) {
                Ok(())
            } else {
                Err(ApplicabilityError)
            }
        }
        Instruction::OptionalSequence {
            instructions: 1, ..
        } => Ok(()),
        Instruction::OptionalSequence { .. } => Err(ApplicabilityError),
        Instruction::CreateTokens { creations } => creations
            .iter()
            .try_for_each(|creation| creation.definition.program().check_current10()),
        Instruction::ZoneOp(spec)
            if spec.as_group_sacrifice().is_some() || spec.as_group_discard().is_some() =>
        {
            Err(ApplicabilityError)
        }
        Instruction::ZoneOp(spec) => {
            if matches!(
                spec.destination,
                spellwake_engine::toolbox::ZoneDest::SwapZones { .. }
            ) || (matches!(spec.source, spellwake_engine::toolbox::ObjectQuery::Targets)
                && matches!(
                    spec.selection,
                    spellwake_engine::toolbox::ZoneSelection::All
                )
                && matches!(spec.destination, spellwake_engine::toolbox::ZoneDest::Hand)
                && spec.shuffle_library.is_none()
                && self.target != spellwake_engine::program::BodyTarget::SINGLE_OWN_GRAVEYARD_CARD)
                || (matches!(
                    &spec.source,
                    spellwake_engine::toolbox::ObjectQuery::ZoneScan {
                        zone: spellwake_engine::toolbox::QueryZone::Library(_),
                        slice: spellwake_engine::toolbox::QuerySlice::Top(_),
                        ..
                    }
                ) && matches!(
                    spec.selection,
                    spellwake_engine::toolbox::ZoneSelection::All
                ) && matches!(
                    spec.destination,
                    spellwake_engine::toolbox::ZoneDest::Exile { .. }
                ) && spec.shuffle_library.is_none())
            {
                Ok(())
            } else {
                Err(ApplicabilityError)
            }
        }
        Instruction::ApplyContinuous(spec) => {
            if spec.condition.is_some() || spec.as_emblem.is_some() {
                return Err(ApplicabilityError);
            }
            match &spec.payload {
                spellwake_engine::toolbox::ContinuousPayload::ResolvedParts(parts)
                    if matches!(
                        spec.selector.as_ref(),
                        Some(spellwake_engine::toolbox::ObjectQuery::Targets)
                    ) =>
                {
                    parts.0.iter().try_for_each(|part| {
                        if matches!(part, ResolvedPartSpec::BlockingRestriction(_)) {
                            return Err(ApplicabilityError);
                        }
                        if let ResolvedPartSpec::Abilities(change) = part {
                            change.check_current10()?;
                        }
                        Ok(())
                    })
                }
                spellwake_engine::toolbox::ContinuousPayload::DoublePt { .. }
                | spellwake_engine::toolbox::ContinuousPayload::PlayPermission { .. }
                | spellwake_engine::toolbox::ContinuousPayload::DamageMultiplier { .. }
                | spellwake_engine::toolbox::ContinuousPayload::ChosenColorProtection { .. } => {
                    Ok(())
                }
                spellwake_engine::toolbox::ContinuousPayload::TargetPreventionShield { .. } => {
                    if matches!(
                        self.target,
                        spellwake_engine::program::BodyTarget::Player {
                            opponents_only: false
                        } | spellwake_engine::program::BodyTarget::AnyTarget(
                            spellwake_engine::program::AnyTargetKind::CreatureOrPlayer
                        )
                    ) {
                        Err(ApplicabilityError)
                    } else {
                        Ok(())
                    }
                }
                _ => Err(ApplicabilityError),
            }
        }
        Instruction::BranchCondition { condition, .. } => match condition.as_ref() {
            spellwake_engine::toolbox::ConditionExpr::Resolution(condition)
                if ![&condition.left, &condition.right].iter().any(|value| {
                    matches!(
                        value,
                        spellwake_engine::program::ValueExpr::EventValue(
                            spellwake_engine::program::EventValueKind::PaidAlternative
                                | spellwake_engine::program::EventValueKind::Bargained
                                | spellwake_engine::program::EventValueKind::KickerCount(_)
                        )
                    )
                }) =>
            {
                Ok(())
            }
            _ => Err(ApplicabilityError),
        },
        Instruction::MutatePlayer(spec)
            if matches!(
                spec.op,
                spellwake_engine::toolbox::PlayerMutation::KeywordAction(
                    spellwake_engine::toolbox::PlayerKeywordAction::CollectEvidence { .. }
                        | spellwake_engine::toolbox::PlayerKeywordAction::Blight { .. }
                        | spellwake_engine::toolbox::PlayerKeywordAction::Populate
                        | spellwake_engine::toolbox::PlayerKeywordAction::Proliferate
                        | spellwake_engine::toolbox::PlayerKeywordAction::Bolster(_)
                        | spellwake_engine::toolbox::PlayerKeywordAction::Amass { .. }
                        | spellwake_engine::toolbox::PlayerKeywordAction::ExtortDrain
                ) | spellwake_engine::toolbox::PlayerMutation::TurnFlow(
                    spellwake_engine::toolbox::TurnFlowOp::AdditionalLoyaltyActivation
                        | spellwake_engine::toolbox::TurnFlowOp::AdditionalCombat { .. }
                ) | spellwake_engine::toolbox::PlayerMutation::LibraryTop(
                    spellwake_engine::toolbox::LibraryTopAction::ManifestTop(_)
                        | spellwake_engine::toolbox::LibraryTopAction::Scry(_)
                        | spellwake_engine::toolbox::LibraryTopAction::Surveil(_)
                        | spellwake_engine::toolbox::LibraryTopAction::Mill(_)
                        | spellwake_engine::toolbox::LibraryTopAction::MayRevealTop
                ) | spellwake_engine::toolbox::PlayerMutation::WatchNextSpellOrLoyalty { .. }
                    | spellwake_engine::toolbox::PlayerMutation::BindResolutionX(_)
                    | spellwake_engine::toolbox::PlayerMutation::OfferSourceCounters { .. }
                    | spellwake_engine::toolbox::PlayerMutation::RequireCondition(_)
                    | spellwake_engine::toolbox::PlayerMutation::ChooseRevealedHandCard { .. }
                    | spellwake_engine::toolbox::PlayerMutation::ExileOnePerPlayerUntilSourceLeaves
                    | spellwake_engine::toolbox::PlayerMutation::SacrificeRandomUntilLand
            ) =>
        {
            Err(ApplicabilityError)
        }
        Instruction::MutateObject(spec)
            if matches!(
                spec.op,
                spellwake_engine::toolbox::ObjectMutation::Counters(
                    spellwake_engine::toolbox::CounterOp::Distribute { .. }
                ) | spellwake_engine::toolbox::ObjectMutation::Explore
                    | spellwake_engine::toolbox::ObjectMutation::StateMarker(
                        spellwake_engine::toolbox::ObjectStateMarkerOp::AdvanceClassLevel(_)
                            | spellwake_engine::toolbox::ObjectStateMarkerOp::SolveCase
                            | spellwake_engine::toolbox::ObjectStateMarkerOp::SetPrepared
                            | spellwake_engine::toolbox::ObjectStateMarkerOp::RememberPermanentTarget
                    )
                    | spellwake_engine::toolbox::ObjectMutation::Combat(
                        spellwake_engine::toolbox::ObjectCombatOp::ReselectAttackDefender
                            | spellwake_engine::toolbox::ObjectCombatOp::SoulbondPair { .. }
                    )
                    | spellwake_engine::toolbox::ObjectMutation::TokenAttach(
                        spellwake_engine::toolbox::TokenAttachOp::CreateAuraAttached { .. }
                            | spellwake_engine::toolbox::TokenAttachOp::CreateAndAttach { .. }
                    )
                    | spellwake_engine::toolbox::ObjectMutation::Ninjutsu
                    | spellwake_engine::toolbox::ObjectMutation::Unearth
                    | spellwake_engine::toolbox::ObjectMutation::Myriad
                    | spellwake_engine::toolbox::ObjectMutation::KeywordZoneAction(
                        spellwake_engine::toolbox::KeywordZoneOp::ScheduleArtifactReturn(_)
                            | spellwake_engine::toolbox::KeywordZoneOp::ExileUnearthed(_)
                            | spellwake_engine::toolbox::KeywordZoneOp::ExileDefeatedSiege
                            | spellwake_engine::toolbox::KeywordZoneOp::SacrificeEvoked
                            | spellwake_engine::toolbox::KeywordZoneOp::SacrificeStateTriggerSource
                    )
                    | spellwake_engine::toolbox::ObjectMutation::ResolveTransfer(_)
                    | spellwake_engine::toolbox::ObjectMutation::ResolveGrowth(_)
                    | spellwake_engine::toolbox::ObjectMutation::ResolveCounterKeyword(_)
                    | spellwake_engine::toolbox::ObjectMutation::ResolveKeywordTrigger(_)
                    | spellwake_engine::toolbox::ObjectMutation::ResolvePowerAbility(_)
                    | spellwake_engine::toolbox::ObjectMutation::Attach(
                        spellwake_engine::toolbox::AttachOp::ToObject {
                            ability: Some(_), ..
                        }
                    )
            ) =>
        {
            Err(ApplicabilityError)
        }
        Instruction::StackOp(spec)
            if matches!(
                spec.op,
                spellwake_engine::toolbox::StackOpKind::Encore
                    | spellwake_engine::toolbox::StackOpKind::Cascade
                    | spellwake_engine::toolbox::StackOpKind::CascadeBottom
                    | spellwake_engine::toolbox::StackOpKind::CopyCapturedAbility
                    | spellwake_engine::toolbox::StackOpKind::CopyOnCast(_)
                    | spellwake_engine::toolbox::StackOpKind::MadnessCast
                    | spellwake_engine::toolbox::StackOpKind::MiracleCast(_)
                    | spellwake_engine::toolbox::StackOpKind::CastDefeatedSiege
                    | spellwake_engine::toolbox::StackOpKind::ReboundCast(_)
                    | spellwake_engine::toolbox::StackOpKind::EndStepCast { .. }
                    | spellwake_engine::toolbox::StackOpKind::ChooseFoundSpell
                    | spellwake_engine::toolbox::StackOpKind::CastFoundSpell(_)
            ) =>
        {
            Err(ApplicabilityError)
        }
        Instruction::DealDamage(spec)
            if matches!(
                spec.recipients,
                spellwake_engine::toolbox::DamageTargetQuery::Divided { .. }
                    | spellwake_engine::toolbox::DamageTargetQuery::FromPower(_)
                    | spellwake_engine::toolbox::DamageTargetQuery::CreaturePowerToOther { .. }
                    | spellwake_engine::toolbox::DamageTargetQuery::DivideThree
                    | spellwake_engine::toolbox::DamageTargetQuery::OrderedTargets { .. }
                    | spellwake_engine::toolbox::DamageTargetQuery::TargetControllersCreatures {
                        ..
                    }
                    | spellwake_engine::toolbox::DamageTargetQuery::FromGrantingPermanent
            ) =>
        {
            Err(ApplicabilityError)
        }
        Instruction::Jump { .. }
        | Instruction::RepeatLoop { .. }
        | Instruction::DrawCards(_)
        | Instruction::MutateObject(_)
        | Instruction::AdjustLife(_)
        | Instruction::DealDamage(_)
        | Instruction::MutatePlayer(_)
        | Instruction::StackOp(_) => Ok(()),
    })
});
native!(TriggeredAbility, self, {
    use spellwake_engine::program::{
        AttackTriggerMode, BlockTriggerKind, CardTypeMask, ControllerFilter, CounterTriggerKind,
        DamageTriggerRecipient, DiesTriggerMode, ObjectFilter, ObjectFilterFlags,
        PlayerTriggerAction, SourceRelation, SpellFilter, StatusTriggerKind, TriggerCondition,
        TriggerEvent, TriggerFromZone, TriggerKeywordEvent, TriggerPlayerScope,
    };
    if matches!(
        &self.condition.event,
        TriggerEvent::SpellCast { filter, self_cast: false, .. }
            if *filter == SpellFilter::historic() || *filter == SpellFilter::colorless()
    ) {
        return Err(ApplicabilityError);
    }
    let excluded_relation = |filter: ObjectFilter| {
        filter == ObjectFilter::any().attached_to_self()
            || filter == ObjectFilter::creature().controlled().nontoken()
            || filter
                == ObjectFilter::creature()
                    .nontoken()
                    .other_than_ability_source()
            || filter
                == ObjectFilter::creature()
                    .controlled()
                    .nontoken()
                    .with_flags(ObjectFilterFlags::WITH_PLUS_ONE_COUNTER)
            || matches!(filter, ObjectFilter::Clause(c) if c.controller == ControllerFilter::You && !c.any_types.is_empty() && c.any_types != CardTypeMask::CREATURE)
            || matches!(filter, ObjectFilter::Either(a, b) if a.source_relation == SourceRelation::SelfObject && b.controller == ControllerFilter::You && !b.any_types.is_empty())
    };
    let excluded = match &self.condition.event {
        TriggerEvent::CountersChanged {
            kind:
                CounterTriggerKind::PlusOnePlaced {
                    by_controller_only: false,
                },
            subject,
        } if *subject != ObjectFilter::self_object() => true,
        TriggerEvent::Dies {
            mode: DiesTriggerMode::Permanent | DiesTriggerMode::Blitz,
            ..
        } => true,
        TriggerEvent::Dies { subject, .. }
        | TriggerEvent::LeavesBattlefield { subject }
        | TriggerEvent::StatusChanged {
            subject,
            kind: StatusTriggerKind::BecomesTapped,
        } if excluded_relation(*subject) => true,
        TriggerEvent::Enters {
            subject,
            from_zone: TriggerFromZone::Any,
            cast_origin: None,
            attached_to_creature: false,
        } if self.condition.intervening_if.is_none() && excluded_relation(*subject) => true,
        TriggerEvent::Enters {
            cast_origin: Some(_),
            ..
        } if !matches!(
            self.condition.event,
            TriggerEvent::Enters {
                cast_origin: Some(spellwake_engine::program::CastEntryKind::Cast { .. }),
                ..
            }
        ) =>
        {
            true
        }
        TriggerEvent::Enters { .. }
            if matches!(
                self.condition.intervening_if,
                Some(TriggerCondition::DuringStep(
                    spellwake_engine::turn::Step::DeclareAttackers
                ))
            ) =>
        {
            true
        }
        TriggerEvent::DealsDamage {
            source,
            recipient: DamageTriggerRecipient::AnyPlayer,
            combat_only: true,
        } => {
            *source == ObjectFilter::any().attached_to_self()
                || matches!(
                    source,
                    ObjectFilter::Clause(c)
                        if c.controller == ControllerFilter::Any
                            && matches!(c.subtypes, spellwake_engine::program::SubtypeFilter::Has(_))
                )
                || self.condition.intervening_if == Some(TriggerCondition::UnrenownedSource)
        }
        TriggerEvent::DealsDamage {
            source,
            recipient: DamageTriggerRecipient::Opponent,
            combat_only: false,
        } if *source == ObjectFilter::self_object() => true,
        TriggerEvent::Attacks {
            subject,
            mode:
                AttackTriggerMode::Individual {
                    capture_defender: true,
                },
        } if *subject == ObjectFilter::self_object() => true,
        TriggerEvent::Attacks {
            subject,
            mode:
                AttackTriggerMode::Individual {
                    capture_defender: false,
                },
        } if *subject == ObjectFilter::any().attached_to_self()
            || *subject
                == ObjectFilter::creature()
                    .controlled()
                    .with_flags(ObjectFilterFlags::WITH_PLUS_ONE_COUNTER) =>
        {
            true
        }
        TriggerEvent::Attacks {
            mode:
                AttackTriggerMode::FirstThisTurn { .. }
                | AttackTriggerMode::WithAtLeastOthers(_)
                | AttackTriggerMode::HighestLifeDefender
                | AttackTriggerMode::WithStrongerCreature
                | AttackTriggerMode::Alone,
            ..
        } => true,
        TriggerEvent::BlocksOrBlocked {
            kind:
                BlockTriggerKind::BecomesBlocked {
                    capture_defender: true,
                }
                | BlockTriggerKind::BlockedByEach {
                    without_flanking_only: false,
                },
            ..
        } => true,
        TriggerEvent::Keyword(
            TriggerKeywordEvent::Increment
            | TriggerKeywordEvent::Graft
            | TriggerKeywordEvent::Evolve
            | TriggerKeywordEvent::Counter(_)
            | TriggerKeywordEvent::Upkeep(_)
            | TriggerKeywordEvent::Soulbond { .. },
        ) => true,
        TriggerEvent::StatusChanged {
            kind: StatusTriggerKind::TurnsFaceUp | StatusTriggerKind::Phased { .. },
            ..
        } => true,
        TriggerEvent::StepBegan { .. }
            if self.condition.intervening_if == Some(TriggerCondition::ControlsMeldPair) =>
        {
            true
        }
        TriggerEvent::Targeted {
            subject,
            by_opponent: false,
            spell_only: true,
            instant_or_sorcery_only: false,
        } if *subject == ObjectFilter::self_object()
            || *subject == ObjectFilter::any().attached_to_self() =>
        {
            true
        }
        TriggerEvent::Targeted {
            subject,
            by_opponent: true,
            spell_only: false,
            instant_or_sorcery_only: false,
        } if *subject == ObjectFilter::self_object() => true,
        TriggerEvent::AnyOf(events)
            if events.iter().any(|e| {
                matches!(
                    e,
                    TriggerEvent::Attacks { .. }
                        | TriggerEvent::StatusChanged {
                            kind: StatusTriggerKind::Transforms,
                            ..
                        }
                )
            }) =>
        {
            true
        }
        TriggerEvent::PutIntoGraveyard {
            from_library: true, ..
        } => true,
        TriggerEvent::DealtDamage {
            linked_player: false,
            ..
        } => true,
        TriggerEvent::PlayerAction {
            action: PlayerTriggerAction::Sacrificed(filter),
            ..
        } if *filter == ObjectFilter::any() => true,
        TriggerEvent::PlayerAction {
            action:
                PlayerTriggerAction::Cycled { .. }
                | PlayerTriggerAction::DrewCard {
                    nth_each_turn: None,
                }
                | PlayerTriggerAction::Scried
                | PlayerTriggerAction::Surveilled,
            ..
        } => true,
        TriggerEvent::SpellCast {
            caster: TriggerPlayerScope::Relation(_),
            ..
        } => false,
        _ => false,
    };
    if excluded {
        return Err(ApplicabilityError);
    }
    self.body.check_current10()
});
fn abilities(
    activated: &[spellwake_engine::activated::ActivatedAbility],
    triggered: &[TriggeredAbility],
) -> Result<(), ApplicabilityError> {
    if activated
        .iter()
        .any(|a| !a.source_cost.is_core_battlefield_cost() && a.source_cost.power_cost().is_none())
    {
        return Err(ApplicabilityError);
    }
    activated
        .iter()
        .try_for_each(|value| value.body.check_current10())?;
    triggered
        .iter()
        .try_for_each(NativeCaptureValue::check_current10)
}
native!(Program, self, {
    if self.meld_partner.is_some()
        || self.split.is_some()
        || self.disturb.is_some()
        || self.modal_back.is_some()
        || self.flip_back.is_some()
        || self.prototype.is_some()
        || self.prepare_spell.is_some()
        || self.enters_prepared
        || !self.static_conditions.is_empty()
    {
        return Err(ApplicabilityError);
    }
    if self.cannot_be_countered || !self.variable_loyalty_costs.is_empty() {
        return Err(ApplicabilityError);
    }
    if !self.artifact_subtypes.is_empty() {
        return Err(ApplicabilityError);
    }
    if !self.enchantment_subtypes.is_empty() {
        return Err(ApplicabilityError);
    }
    if self.cast_during_own_combat
        || self.combat_limits != Default::default()
        || self.attack_requirement.is_some()
        || self.additional_life_cost.is_some()
        || self.x_mana_symbols != 0
    {
        return Err(ApplicabilityError);
    }
    if self.has_type(spellwake_engine::program::CardType::Planeswalker)
        || self.printed_defense.is_some()
        || self.siege_back.is_some()
        || self.printed_loyalty.is_some()
        || !self.loyalty_costs.is_empty()
        || !self.planeswalker_subtypes.is_empty()
    {
        return Err(ApplicabilityError);
    }
    if matches!(&self.kind, ProgramKind::Spell(spell) if spell.modes.is_some()) {
        return Err(ApplicabilityError);
    }

    if !self.face_up_abilities.is_empty() {
        return Err(ApplicabilityError);
    }
    if !self.activation_restrictions.is_empty() {
        return Err(ApplicabilityError);
    }
    if self.plot.is_some()
        || self.conditional_cost.is_some()
        || !self.additional_cast_costs.is_empty()
        || self.reveal_colorless_creature_cast_cost
        || self.sacrifice_creature_cast_cost
        || self.tap_creatures_cast_cost
        || self.discard_cast_cost
        || self.flashback.is_some()
        || !self.entry_counter_abilities.is_empty()
        || !self.cast_reductions.is_empty()
        || self.printed_toxic.is_some()
        || !self.printed_keywords.is_empty()
        || !self.zone_replacements.is_empty()
    {
        return Err(ApplicabilityError);
    }
    match &self.kind {
        ProgramKind::Spell(spell) => spell.body.check_current10()?,
        ProgramKind::Permanent(value) if value.attachment.is_some() => {
            return Err(ApplicabilityError);
        }
        ProgramKind::Permanent(_) => {}
    }
    if !self.additional_battlefield_abilities.is_empty() {
        return Err(ApplicabilityError);
    }
    self.continuous.check_current10()?;
    abilities(&self.activated, &self.triggered)
});
native!(PrintedText, self, {
    if !self.face_up_abilities.is_empty() {
        return Err(ApplicabilityError);
    }
    if !self.activation_restrictions.is_empty() {
        return Err(ApplicabilityError);
    }
    if self.plot.is_some()
        || self.conditional_cost.is_some()
        || !self.additional_cast_costs.is_empty()
        || self.reveal_colorless_creature_cast_cost
        || self.sacrifice_creature_cast_cost
        || self.tap_creatures_cast_cost
        || self.discard_cast_cost
        || self.flashback.is_some()
        || !self.entry_counter_abilities.is_empty()
        || !self.cast_reductions.is_empty()
        || self.printed_toxic.is_some()
        || !self.printed_keywords.is_empty()
        || !self.zone_replacements.is_empty()
    {
        return Err(ApplicabilityError);
    }
    if !self.additional_battlefield_abilities.is_empty() {
        return Err(ApplicabilityError);
    }
    self.continuous.check_current10()?;
    abilities(&self.activated, &self.triggered)
});
native!(GameSeed, self, {
    self.cards
        .iter()
        .try_for_each(|value| value.program.check_current10())
});
native!(GameObject, self, {
    if self.face_down().is_some() {
        return Err(ApplicabilityError);
    }
    match self.origin() {
        ObjectOrigin::Card(_) => Ok(()),
        ObjectOrigin::PermanentSpellCopy { .. } | ObjectOrigin::SpellCopy { .. } => {
            Err(ApplicabilityError)
        }
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
        StackKind::Speed
        | StackKind::Monarch
        | StackKind::DelayedCopy { .. }
        | StackKind::Emblem { .. } => Err(ApplicabilityError),
        StackKind::CardSpell(_) | StackKind::SpellCopy { .. } => Ok(()),
    }
});
native!(StackWindow, self, {
    if self.entries().iter().chain(self.resolving()).any(|entry| {
        entry.paid_conditional_cost()
            || entry.cast_with_flashback()
            || entry.cast_with_jump_start()
            || entry.bought_back()
            || !entry.kicker_choices().is_empty()
    }) {
        return Err(ApplicabilityError);
    }
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
        ResolvedPart::InheritedAbilities(_)
        | ResolvedPart::GrantActivated(_)
        | ResolvedPart::GrantActivatedRestricted { .. }
        | ResolvedPart::GrantTrigger(_)
        | ResolvedPart::GrantAnyColorMana
        | ResolvedPart::GrantFixedColorMana(_)
        | ResolvedPart::GrantMana(_)
        | ResolvedPart::TypeWithSubtypes { .. }
        | ResolvedPart::BlockingRestriction(_)
        | ResolvedPart::GrantRestriction(_)
        | ResolvedPart::CompoundAbilities(_) => Err(ApplicabilityError),
        ResolvedPart::Abilities(change) => change.check_current10(),
        ResolvedPart::Control(_)
        | ResolvedPart::Text(_)
        | ResolvedPart::Names(_)
        | ResolvedPart::Type(_)
        | ResolvedPart::Color(_)
        | ResolvedPart::CannotBeTargetedByColors(_)
        | ResolvedPart::CannotBeTargetedBySpellsOfColors(_)
        | ResolvedPart::SetPT(_)
        | ResolvedPart::SetPower(_)
        | ResolvedPart::SetToughness(_)
        | ResolvedPart::ModifyPT(_)
        | ResolvedPart::GrantLoyalty { .. }
        | ResolvedPart::SwitchPT => Ok(()),
    })
});
native!(SupportedAppearance, self, {
    if !self.artifact_subtypes.is_empty() || !self.copiable_artifact_subtypes.is_empty() {
        return Err(ApplicabilityError);
    }
    if !self.enchantment_subtypes.is_empty() || !self.copiable_enchantment_subtypes.is_empty() {
        return Err(ApplicabilityError);
    }
    if self.saddled || self.attached_to.is_some() {
        return Err(ApplicabilityError);
    }
    if self.time_counters.is_some()
        || self.fade_counters.is_some()
        || self.previous_upkeep.is_some()
        || self.renowned
        || self.toxic_value.is_some()
        || !self.granted_triggers.is_empty()
        || self.granted_unblockable
        || !self.granted_exalted.is_empty()
        || !self.granted_combat_keywords.is_empty()
        || !self.abilities.zone_replacements.is_empty()
    {
        return Err(ApplicabilityError);
    }
    historical_keywords(&self.keywords)?;
    self.object.check_current10()?;
    self.printed_text.check_current10()
});
native!(TriggerCauseSet, self, {
    self.members()
        .iter()
        .try_for_each(|member| match &member.related {
            TriggerRelated::CombatDamageToPlaneswalker { .. }
            | TriggerRelated::SubtypeCombatDamage { .. }
            | TriggerRelated::AttachedCreatureDamaged { .. }
            | TriggerRelated::DamageToOpponent { .. }
            | TriggerRelated::GraveyardReturnPaid { .. }
            | TriggerRelated::ReflexiveDiscardDamage { .. }
            | TriggerRelated::DamageReceived { .. }
            | TriggerRelated::CombatDamageToPlayer { .. }
            | TriggerRelated::AbilityForCopy(_)
            | TriggerRelated::SpellCopied(_)
            | TriggerRelated::TargetingEntry(_)
            | TriggerRelated::TargetedPermanent(_)
            | TriggerRelated::DamageSource(_)
            | TriggerRelated::StateTriggerSource(_)
            | TriggerRelated::FinalSagaChapter { .. }
            | TriggerRelated::ManaSource(_)
            | TriggerRelated::AttackGroup { .. }
            | TriggerRelated::CardDrawn { .. }
            | TriggerRelated::PlayerLost(_)
            | TriggerRelated::SagaChapter { .. }
            | TriggerRelated::CounterSource(_)
            | TriggerRelated::Combatant(_)
            | TriggerRelated::LeftGame(_)
            | TriggerRelated::DeclaredSubtypeAttackers(_)
            | TriggerRelated::AttackedWithTotalPower(_)
            | TriggerRelated::AttackedAlone(_)
            | TriggerRelated::LibraryAction(_)
            | TriggerRelated::Mutated(_)
            | TriggerRelated::Phased(_) => Err(ApplicabilityError),
            TriggerRelated::SpellCast(value) => value.spell().check_current10(),
            TriggerRelated::TokenCreated { after, .. } => after.check_current10(),
            TriggerRelated::ZoneChange { before, after }
            | TriggerRelated::Discard { before, after, .. } => {
                before.check_current10()?;
                after.check_current10()
            }
            TriggerRelated::StatusChange(value) => value.check_current10(),
            TriggerRelated::LifeChanged(change) if change.lifelink.is_some() => {
                Err(ApplicabilityError)
            }
            TriggerRelated::LifeChanged(_)
            | TriggerRelated::MiracleRevealed(_)
            | TriggerRelated::Step { .. }
            | TriggerRelated::DamageDealtToPermanent { .. }
            | TriggerRelated::ReflexiveTrigger { .. } => Ok(()),
        })
});
native!(WaitingInstance, self, {
    if self.granting_effect.is_some() {
        return Err(ApplicabilityError);
    }
    self.body.check_current10()?;
    self.source.check_current10()?;
    self.causes.check_current10()
});
native!(TriggerBinding, self, {
    if self.granting_effect.is_some() {
        return Err(ApplicabilityError);
    }
    self.source.check_current10()?;
    self.causes.check_current10()
});
native!(ExecutionBindings, self, { self.trigger.check_current10() });
native!(RetainedStackPayload<'_>, self, {
    if self.copied_keywords.is_some_and(|keys| !keys.is_empty()) {
        return Err(ApplicabilityError);
    }
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
    if self.emblems().next().is_some() || !self.loyalty_counters().is_empty() {
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
    if !self.growth_last_known().is_empty()
        || !self.time_counters().is_empty()
        || !self.fade_counters().is_empty()
        || self.upkeep_cuts().is_some()
        || !self.age_counters().is_empty()
        || self.damage_to_players_this_turn().is_some()
        || self.cast_turn_history().is_some()
        || !self.renowned_objects().is_empty()
        || !self.poisoned_players().is_empty()
        || self.has_check_replacement_work()
        || !self.deathtouch_damage().is_empty()
        || !self.departed_damage_sources().is_empty()
        || self
            .combat()
            .is_some_and(|combat| !combat.first_strike_participants().is_empty())
    {
        return Err(ApplicabilityError);
    }
    match self.pending_work_kind() {
        Some(
            PendingWorkKind::LibraryTop
            | PendingWorkKind::LookTop
            | PendingWorkKind::ResolutionSelection
            | PendingWorkKind::BattlefieldEntry
            | PendingWorkKind::PermanentEntry
            | PendingWorkKind::SearchEntry
            | PendingWorkKind::LandPlay
            | PendingWorkKind::ZoneMove
            | PendingWorkKind::ActivationCost
            | PendingWorkKind::CastRecovery,
        ) => Err(ApplicabilityError),
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
native!(CombatDecision, self, {
    match self {
        CombatDecision::Blockers(frame)
            if frame.restricted_blockers().next().is_some()
                || frame.group_minimums().next().is_some() =>
        {
            Err(ApplicabilityError)
        }
        CombatDecision::DamageAssignments(frame)
            if frame.position().additional_damage
                || frame
                    .domains()
                    .iter()
                    .any(|domain| !domain.trample_lethal().is_empty()) =>
        {
            Err(ApplicabilityError)
        }
        _ => Ok(()),
    }
});
native!(RuleFrame, self, {
    match self {
        RuleFrame::CastRecovery(_) => Err(ApplicabilityError),
        RuleFrame::EntryChoice(_)
        | RuleFrame::TokenEntryChoice(_)
        | RuleFrame::TokenAuraChoice(_)
        | RuleFrame::LinkedReturnOrder(_)
        | RuleFrame::LinkedAuraChoice(_) => Err(ApplicabilityError),
        RuleFrame::Checks(
            spellwake_engine::checks::CheckDecision::CleanupSelect(_)
            | spellwake_engine::checks::CheckDecision::BattleProtector(_)
            | spellwake_engine::checks::CheckDecision::ReturnCommanders(_),
        ) => Err(ApplicabilityError),
        RuleFrame::ZoneReplacement(_) => Err(ApplicabilityError),
        RuleFrame::LibraryTop(_) => Err(ApplicabilityError),
        RuleFrame::ResolutionSelect(_) | RuleFrame::ResolutionGraveyardOrder(_) => {
            Err(ApplicabilityError)
        }
        RuleFrame::CounterReplacement(_) => Err(ApplicabilityError),
        RuleFrame::TransformOrder(_) => Err(ApplicabilityError),
        RuleFrame::Mutation(_)
        | RuleFrame::RevealedPiles(_)
        | RuleFrame::DrawReplacement(_)
        | RuleFrame::DamageSourceChoice(_)
        | RuleFrame::OpponentChoice(_)
        | RuleFrame::ReplacementChooser(_)
        | RuleFrame::MyriadDefender(_)
        | RuleFrame::ReselectAttackDefender(_)
        | RuleFrame::TokenDefender(_) => Err(ApplicabilityError),
        RuleFrame::TimestampOrder(value) => value
            .members()
            .iter()
            .try_for_each(|member| member.definition.check_current10()),
        RuleFrame::Combat(value) => value.check_current10(),
        RuleFrame::Damage(value) => value.check_current10(),
        RuleFrame::OptionalEffect(_)
        | RuleFrame::ResolutionPayment(_)
        | RuleFrame::Checks(_)
        | RuleFrame::Triggers(_)
        | RuleFrame::TriggerTarget(_)
        | RuleFrame::Opening(_)
        | RuleFrame::CopyTargets(_)
        | RuleFrame::ManaColor(_)
        | RuleFrame::CreatureType(_) => Ok(()),
        RuleFrame::PromptChoice(frame)
            if matches!(
                frame.domain,
                spellwake_engine::toolbox::ResolvedPromptDomain::CreatureType { .. }
            ) =>
        {
            Ok(())
        }
        RuleFrame::ResolutionManaColor(_)
        | RuleFrame::OptionIndex(_)
        | RuleFrame::PromptChoice(_)
        | RuleFrame::Proliferate(_)
        | RuleFrame::UntapKeepTapped(_) => Err(ApplicabilityError),
    }
});
native!(EffectDecision, self, {
    match self {
        EffectDecision::ManaColor(_) | EffectDecision::CastRecovery(_) => Err(ApplicabilityError),
        EffectDecision::EntryChoice(_)
        | EffectDecision::TokenEntryChoice(_)
        | EffectDecision::TokenAuraChoice(_)
        | EffectDecision::LinkedReturnOrder(_)
        | EffectDecision::LinkedAuraChoice(_) => Err(ApplicabilityError),
        EffectDecision::Checks(
            spellwake_engine::checks::CheckDecision::CleanupSelect(_)
            | spellwake_engine::checks::CheckDecision::BattleProtector(_)
            | spellwake_engine::checks::CheckDecision::ReturnCommanders(_),
        ) => Err(ApplicabilityError),
        EffectDecision::ZoneReplacement(_) => Err(ApplicabilityError),
        EffectDecision::LibraryTop(_) => Err(ApplicabilityError),
        EffectDecision::ResolutionSelect(_) | EffectDecision::ResolutionGraveyardOrder(_) => {
            Err(ApplicabilityError)
        }
        EffectDecision::Mutation(_)
        | EffectDecision::RevealedPiles(_)
        | EffectDecision::DrawReplacement(_)
        | EffectDecision::DamageSourceChoice(_)
        | EffectDecision::OpponentChoice(_)
        | EffectDecision::ReplacementChooser(_)
        | EffectDecision::MyriadDefender(_)
        | EffectDecision::ReselectAttackDefender(_)
        | EffectDecision::TokenDefender(_) => Err(ApplicabilityError),
        EffectDecision::TransformOrder(_) => Err(ApplicabilityError),
        EffectDecision::TimestampOrder(value) => value
            .members()
            .iter()
            .try_for_each(|member| member.definition.check_current10()),
        EffectDecision::Combat(value) => value.check_current10(),
        EffectDecision::Damage(value) => value.check_current10(),
        EffectDecision::OptionalEffect(_)
        | EffectDecision::ResolutionPayment(_)
        | EffectDecision::Checks(_)
        | EffectDecision::Triggers(_)
        | EffectDecision::TriggerTarget(_)
        | EffectDecision::CopyTargets(_)
        | EffectDecision::CreatureType(_) => Ok(()),
        EffectDecision::PromptChoice(frame)
            if matches!(
                frame.domain,
                spellwake_engine::toolbox::ResolvedPromptDomain::CreatureType { .. }
            ) =>
        {
            Ok(())
        }
        EffectDecision::ResolutionManaColor(_)
        | EffectDecision::OptionIndex(_)
        | EffectDecision::PromptChoice(_)
        | EffectDecision::Proliferate(_)
        | EffectDecision::UntapKeepTapped(_) => Err(ApplicabilityError),
    }
});
native!(GameState, self, {
    if self.conclusion().is_some_and(|c| {
        matches!(
            c.cause(),
            spellwake_engine::game::GameConclusionCause::Conceded { .. }
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
        DecisionReply::PayAttackCost { .. }
        | DecisionReply::RedeclareReversingMana(_)
        | DecisionReply::RedeclareKeepingMana
        | DecisionReply::RedeclareAttackers => Err(ApplicabilityError),
        DecisionReply::TriggerPermanentTargets(_) | DecisionReply::CastRecovery(_) => {
            Err(ApplicabilityError)
        }
        DecisionReply::EntryChoice(_)
        | DecisionReply::TokenEntryChoice(_)
        | DecisionReply::TokenAuraChoice(_)
        | DecisionReply::LinkedReturnOrder(_)
        | DecisionReply::LinkedAuraChoice(_) => Err(ApplicabilityError),
        DecisionReply::PromptChoice(
            spellwake_engine::toolbox::PromptChoiceReply::CreatureType(_),
        ) => Ok(()),
        DecisionReply::TransformOrder(_) | DecisionReply::PromptChoice(_) => {
            Err(ApplicabilityError)
        }
        DecisionReply::ChooseLandKind(_)
        | DecisionReply::MutateOnTop(_)
        | DecisionReply::ChoosePile(_)
        | DecisionReply::DrawReplacement(_)
        | DecisionReply::ChooseDamageSource(_)
        | DecisionReply::OpponentChoice(_)
        | DecisionReply::ReplacementChooser(_)
        | DecisionReply::MyriadDefender(_)
        | DecisionReply::ReselectAttackDefender(_)
        | DecisionReply::TokenDefender(_) => Err(ApplicabilityError),
        DecisionReply::ReturnCommanders(_) | DecisionReply::ZoneReplacement(_) => {
            Err(ApplicabilityError)
        }
        DecisionReply::LibraryTop(_) => Err(ApplicabilityError),
        DecisionReply::SelectResolution(_) | DecisionReply::OrderResolutionGraveyard(_) => {
            Err(ApplicabilityError)
        }
        DecisionReply::ImpulseCast { .. } => Err(ApplicabilityError),
        DecisionReply::CascadeCast { .. }
        | DecisionReply::MadnessCast { .. }
        | DecisionReply::ReboundCast { .. }
        | DecisionReply::SuspendCast { .. }
        | DecisionReply::OptionalEffect(_)
        | DecisionReply::ResolutionPayment(_)
        | DecisionReply::TimestampOrder(_)
        | DecisionReply::Discard(_)
        | DecisionReply::OrderGraveyard(_)
        | DecisionReply::KeepLegend { .. }
        | DecisionReply::OrderTriggers(_)
        | DecisionReply::TriggerTarget(_)
        | DecisionReply::DeclareBandedAttackers { .. }
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
        DecisionReply::Proliferate(_)
        | DecisionReply::OptionIndex(_)
        | DecisionReply::SplitMana(_)
        | DecisionReply::TriggerModes(_)
        | DecisionReply::ApplyCounterDoublersFirst(_)
        | DecisionReply::UntapKeepTapped(_)
        | DecisionReply::OpeningBattlefield(_) => Err(ApplicabilityError),
    }
});
native!(GameInput, self, {
    match &self.command {
        GameCommand::ActivateManaWithRestrictedPayment { .. }
        | GameCommand::ActivateWithRestrictedMana { .. }
        | GameCommand::ActivateManaWithCostObjects { .. }
        | GameCommand::ActivateManaForPlayer { .. }
        | GameCommand::ActivateWithCostObjects { .. }
        | GameCommand::ActivateEmblem { .. }
        | GameCommand::ActivatePermanentTargets { .. }
        | GameCommand::ActivateGranted { .. }
        | GameCommand::ActivateModalAbility { .. }
        | GameCommand::ActivateLoyaltyX { .. } => Err(ApplicabilityError),
        GameCommand::ActivateManaWithBundle { .. }
        | GameCommand::PlayLandWithEntryColor { .. }
        | GameCommand::PlayLandWithEntryReveal { .. }
        | GameCommand::ActivateHybridMana { .. }
        | GameCommand::ActivateManaWithPayment { .. }
        | GameCommand::CastFaceDown { .. }
        | GameCommand::TurnFaceUp { .. }
        | GameCommand::Foretell { .. }
        | GameCommand::Suspend { .. }
        | GameCommand::Plot { .. }
        | GameCommand::CastWithResources { .. }
        | GameCommand::Concede
        | GameCommand::ActivateWithCreatures { .. }
        | GameCommand::ActivateUntargetedAbility { .. }
        | GameCommand::Cycle { .. }
        | GameCommand::Ninjutsu { .. }
        | GameCommand::Transmute { .. } => Err(ApplicabilityError),
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
        RulesRecord::AttackDeclarationReversed { .. }
        | RulesRecord::CastAnnouncementReversed { .. } => return Err(ApplicabilityError),
        RulesRecord::GameConcluded { .. } => return Err(ApplicabilityError),
        RulesRecord::Event(event) => {
            if matches!(
                event.cause,
                EventCause::TurnFaceUp { .. }
                    | EventCause::Foretell { .. }
                    | EventCause::Suspend { .. }
                    | EventCause::Plot { .. }
                    | EventCause::CastLifePayment { .. }
                    | EventCause::EscapePayment { .. }
                    | EventCause::CastObjectPayment { .. }
            ) {
                return Err(ApplicabilityError);
            }
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
        RulesRecord::DelayedCopyTriggerDiscovered { .. }
        | RulesRecord::EmblemTriggerDiscovered { .. }
        | RulesRecord::SpeedTriggerDiscovered { .. }
        | RulesRecord::MonarchTriggerDiscovered { .. } => return Err(ApplicabilityError),
        RulesRecord::TriggerDiscovered(value) => value.check_current10()?,
        RulesRecord::ChooserReplaced { .. } => return Err(ApplicabilityError),
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
        RulesOccurrence::AttackDefenderChanged { .. }
        | RulesOccurrence::AttackCostPaid { .. }
        | RulesOccurrence::Mutated { .. }
        | RulesOccurrence::ClassLevelReached { .. }
        | RulesOccurrence::CaseSolved { .. }
        | RulesOccurrence::PreparedSpellCast { .. }
        | RulesOccurrence::Phased { .. }
        | RulesOccurrence::MiracleRevealed { .. }
        | RulesOccurrence::HandCardRevealed { .. }
        | RulesOccurrence::Explored { .. }
        | RulesOccurrence::LibraryCardRevealed { .. } => Err(ApplicabilityError),
        RulesOccurrence::LoreCountersChanged { .. }
        | RulesOccurrence::CounterThresholdMet { .. }
        | RulesOccurrence::SubtypeThresholdMet { .. }
        | RulesOccurrence::PermanentCounterChanged { .. }
        | RulesOccurrence::SuspectCountersChanged { .. }
        | RulesOccurrence::ChargeCountersChanged { .. }
        | RulesOccurrence::StorageCountersChanged { .. } => Err(ApplicabilityError),
        RulesOccurrence::LoyaltyCountersChanged { .. } => Err(ApplicabilityError),
        RulesOccurrence::LibraryAction(_) => Err(ApplicabilityError),
        RulesOccurrence::TokenCreated { after, .. } => after.check_current10(),
        RulesOccurrence::TokenCeased { object } => object.check_current10(),
        RulesOccurrence::ControllerChanged(value) => value.check_current10(),
        RulesOccurrence::ZoneChange(value) => {
            if !value.additional_arrivals.is_empty() {
                return Err(ApplicabilityError);
            }
            match value.reason {
                ZoneReason::TransmuteDiscard { .. }
                | ZoneReason::CascadeExile
                | ZoneReason::CascadeBottom
                | ZoneReason::PutIntoOwnersLibrary(_)
                | ZoneReason::CommanderReturn
                | ZoneReason::PlayerDeparture
                | ZoneReason::MadnessDeclined
                | ZoneReason::PersistReturn
                | ZoneReason::UndyingReturn
                | ZoneReason::NinjutsuEntry
                | ZoneReason::UnearthReturn
                | ZoneReason::DashReturn
                | ZoneReason::WarpExile
                | ZoneReason::Ingest { .. }
                | ZoneReason::UnearthExile
                | ZoneReason::SearchToExile
                | ZoneReason::TransfigureEntry
                | ZoneReason::SearchToHand
                | ZoneReason::LookTopToHand
                | ZoneReason::Foretell
                | ZoneReason::Suspend
                | ZoneReason::Plot
                | ZoneReason::AlternativeHandExileCost
                | ZoneReason::EscapeCost
                | ZoneReason::DelveCost
                | ZoneReason::CycleDiscard { .. } => {
                    return Err(ApplicabilityError);
                }
                ZoneReason::StateBased(reasons)
                    if reasons.contains(
                        spellwake_engine::occurrence::StateBasedMoveReason::IllegalAura,
                    ) || reasons.contains(
                        spellwake_engine::occurrence::StateBasedMoveReason::DeathtouchDestruction,
                    ) =>
                {
                    return Err(ApplicabilityError);
                }
                ZoneReason::ReturnFromSacrifice
                | ZoneReason::ReturnFromLinkedExile
                | ZoneReason::ReturnFromGraveyardToBattlefield
                | ZoneReason::Manifest
                | ZoneReason::ExileToBattlefield
                | ZoneReason::HandToBattlefield
                | ZoneReason::LookTopToBattlefield
                | ZoneReason::SearchToBattlefield
                | ZoneReason::ShufflePermanentIntoLibrary
                | ZoneReason::ExiledCardsToLibraryBottom
                | ZoneReason::ShuffleHandAndGraveyardIntoLibrary
                | ZoneReason::ShuffleGraveyardIntoLibrary
                | ZoneReason::RevealedToGraveyard
                | ZoneReason::PutLibraryCohortIntoGraveyard
                | ZoneReason::SearchToGraveyard
                | ZoneReason::ExiledByEffect
                | ZoneReason::ExileCost
                | ZoneReason::Destroyed
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
        RulesOccurrence::StatusChange {
            reason: spellwake_engine::occurrence::StatusReason::CastObjectCost,
            ..
        } => Err(ApplicabilityError),
        RulesOccurrence::StatusChange { change, .. } => change.check_current10(),
        RulesOccurrence::TimeCounterRemoved { .. }
        | RulesOccurrence::FadeCounterRemoved { .. }
        | RulesOccurrence::PermanentCountersDoubled { .. }
        | RulesOccurrence::Proliferated { .. }
        | RulesOccurrence::AgeCounterPlaced { .. }
        | RulesOccurrence::PermanentTargeted { .. }
        | RulesOccurrence::PlayerTargeted { .. }
        | RulesOccurrence::CommanderTypeManaSpent { .. }
        | RulesOccurrence::ReflexiveDiscardDamage { .. }
        | RulesOccurrence::ManaAddedByEffect { .. }
        | RulesOccurrence::EmblemActivated { .. }
        | RulesOccurrence::EmblemCreated { .. }
        | RulesOccurrence::EmblemDamagePrevented { .. }
        | RulesOccurrence::StaticDamagePrevented { .. }
        | RulesOccurrence::DamageReplacedWithMill { .. }
        | RulesOccurrence::StaticDamageMultiplied { .. }
        | RulesOccurrence::TurnPreventionCreated { .. }
        | RulesOccurrence::TurnPreventionSpecCreated { .. }
        | RulesOccurrence::TurnDamagePrevented { .. }
        | RulesOccurrence::RegenerationShieldCreated { .. }
        | RulesOccurrence::PermanentRegenerated { .. }
        | RulesOccurrence::CreaturesGoaded { .. }
        | RulesOccurrence::AdditionalCombatCreated
        | RulesOccurrence::ExtraTurnCreated { .. }
        | RulesOccurrence::SpellControllerChanged { .. }
        | RulesOccurrence::StackTargetsChanged { .. }
        | RulesOccurrence::AbilityCountered { .. }
        | RulesOccurrence::SpeedChanged { .. }
        | RulesOccurrence::MonarchChanged { .. }
        | RulesOccurrence::CityBlessingGained { .. }
        | RulesOccurrence::CardDrawOrdinal { .. }
        | RulesOccurrence::PowerAbilityResolved { .. }
        | RulesOccurrence::DayNightChanged { .. }
        | RulesOccurrence::Transformed { .. }
        | RulesOccurrence::TurnedFaceUpByEffect { .. }
        | RulesOccurrence::TurnedFaceUp { .. }
        | RulesOccurrence::FaceDownRevealed { .. }
        | RulesOccurrence::AttachmentChanged { .. } => Err(ApplicabilityError),
        RulesOccurrence::CastCompleted(value) => value.spell().check_current10(),
        RulesOccurrence::ActivatedAbility { source, .. }
        | RulesOccurrence::ManaAbility { source, .. } => source.check_current10(),
        RulesOccurrence::AbilityCopied { .. } => return Err(ApplicabilityError),
        RulesOccurrence::CopyCreated {
            copiable_triggered,
            copiable_keywords,
            copiable_artifact_subtypes,
            copiable_enchantment_subtypes,
            ..
        } => {
            if !copiable_keywords.is_empty()
                || !copiable_artifact_subtypes.is_empty()
                || !copiable_enchantment_subtypes.is_empty()
            {
                return Err(ApplicabilityError);
            }
            copiable_triggered.check_current10()
        }
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
        RulesOccurrence::FinalSagaChapterCompleted { .. } => Err(ApplicabilityError),
        RulesOccurrence::EntryCompleted { kind, .. } => kind.check_current10(),
        RulesOccurrence::DamageDealt { damage, .. } => damage.source.check_current10(),
        RulesOccurrence::LifeChanged(change) if change.lifelink.is_some() => {
            Err(ApplicabilityError)
        }
        RulesOccurrence::PlayerLost { reasons, .. }
            if reasons.iter().any(|reason| {
                matches!(
                    reason,
                    spellwake_engine::occurrence::PlayerLossReason::TenPoisonCounters
                        | spellwake_engine::occurrence::PlayerLossReason::CommanderCombatDamage(_)
                )
            }) =>
        {
            Err(ApplicabilityError)
        }
        RulesOccurrence::TurnsSkipped { .. }
        | RulesOccurrence::PermanentLeftGame { .. }
        | RulesOccurrence::SourceControlEffectsEnded { .. }
        | RulesOccurrence::PlayerProtectionGranted { .. }
        | RulesOccurrence::ProtectionPrevented(_)
        | RulesOccurrence::PowerToughnessCountersRemoved { .. }
        | RulesOccurrence::CounterRemovedForGraft { .. }
        | RulesOccurrence::CreatureEvolved { .. }
        | RulesOccurrence::CreatureMentored { .. }
        | RulesOccurrence::BecameRenowned { .. }
        | RulesOccurrence::ExperienceCountersChanged { .. }
        | RulesOccurrence::PlayerPoisoned { .. } => Err(ApplicabilityError),
        RulesOccurrence::LifeChanged(_)
        | RulesOccurrence::CreatureTypeChosen { .. }
        | RulesOccurrence::StackExiledToEndTurn { .. }
        | RulesOccurrence::GraveyardReturnPaid { .. }
        | RulesOccurrence::CardVoteCast { .. }
        | RulesOccurrence::CoinFlipped { .. }
        | RulesOccurrence::Shuffled(_)
        | RulesOccurrence::FailedDraw { .. }
        | RulesOccurrence::ManaProduced { .. }
        | RulesOccurrence::ManaDebited { .. }
        | RulesOccurrence::ManaExpired { .. }
        | RulesOccurrence::ArtifactTappedForMana { .. }
        | RulesOccurrence::PermanentTappedForMana { .. }
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
        | RulesOccurrence::PlayerCounterChanged { .. }
        | RulesOccurrence::StepBegan { .. }
        | RulesOccurrence::ReflexiveTrigger { .. }
        | RulesOccurrence::StateConditionMet { .. }
        | RulesOccurrence::KeywordActionPerformed { .. }
        | RulesOccurrence::DoorUnlocked { .. }
        | RulesOccurrence::DieRolled { .. }
        | RulesOccurrence::VotesCast { .. }
        | RulesOccurrence::BecameMonstrous { .. }
        | RulesOccurrence::PermanentFought { .. }
        | RulesOccurrence::ChoiceMade { .. } => Ok(()),
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
                | spellwake_engine::checks::CheckChoice::BattleProtector { .. }
                | spellwake_engine::checks::CheckChoice::ReturnCommanders { .. }
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
        spellwake_engine::damage::DamageSource::Emblem { .. } => Err(ApplicabilityError),
        spellwake_engine::damage::DamageSource::Object { object, .. } => object.check_current10(),
        spellwake_engine::damage::DamageSource::SpellCopy { .. } => Ok(()),
    }
});
native!(spellwake_engine::damage::DamageJournal, self, {
    if !self.protections.is_empty() {
        return Err(ApplicabilityError);
    }
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
        spellwake_engine::effects::DamageDecision::Order(order)
            if order.applications().iter().any(|a| {
                matches!(
                    a.effect,
                    spellwake_engine::damage::DamageEffectId::EmblemCap { .. } | spellwake_engine::damage::DamageEffectId::StaticPrevention(_) | spellwake_engine::damage::DamageEffectId::StaticPreventionWithExile(_, _) | spellwake_engine::damage::DamageEffectId::CombatDamageToMill(_, _) | spellwake_engine::damage::DamageEffectId::RemovePlusOneToPreventDamage(_, _) | spellwake_engine::damage::DamageEffectId::StaticDouble(_) | spellwake_engine::damage::DamageEffectId::DoubleDamageFromEquippedCreature(_) | spellwake_engine::damage::DamageEffectId::DoubleDamageToEquippedCreature(_) | spellwake_engine::damage::DamageEffectId::StaticHalfPrevention { .. } | spellwake_engine::damage::DamageEffectId::ProtectionMulticolored | spellwake_engine::damage::DamageEffectId::ProtectionSubtype(_) | spellwake_engine::damage::DamageEffectId::Protection(_) | spellwake_engine::damage::DamageEffectId::UntilEndOfTurn { .. }
                )
            }) =>
        {
            Err(ApplicabilityError)
        }
        spellwake_engine::effects::DamageDecision::Order(_) => Ok(()),
    }
});

#[cfg(test)]
#[path = "applicability/tests.rs"]
pub(crate) mod tests;
