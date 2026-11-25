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
    if self.granted_trigger.is_some()
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
    if matches!(self.target,spellwake_engine::program::BodyTarget::ControlledAbility {..} | spellwake_engine::program::BodyTarget::SinglePlayerOrPlaneswalker | spellwake_engine::program::BodyTarget::StackEntryTargetingControlledPermanent | spellwake_engine::program::BodyTarget::SingleStackEntry | spellwake_engine::program::BodyTarget::SinglePlaneswalker | spellwake_engine::program::BodyTarget::SingleAnyTarget | spellwake_engine::program::BodyTarget::FilteredPermanent(_)
        | spellwake_engine::program::BodyTarget::AnyGraveyardCreature
        | spellwake_engine::program::BodyTarget::AnyGraveyardCard
        | spellwake_engine::program::BodyTarget::AnyGraveyardCreatureOrPlaneswalker
        | spellwake_engine::program::BodyTarget::FilteredOwnGraveyardCard(_)
        | spellwake_engine::program::BodyTarget::FilteredAnyGraveyardCard { .. }
        | spellwake_engine::program::BodyTarget::SingleOpponentOrPlaneswalker
        | spellwake_engine::program::BodyTarget::SingleSpell(_)
            if !matches!(self.target, spellwake_engine::program::BodyTarget::SingleSpell(f)
                if f != spellwake_engine::program::SpellFilter::creature()
                    && f != spellwake_engine::program::SpellFilter::noncreature()))
    {
        return Err(ApplicabilityError);
    }
    if self.triggered_modes().is_some() {
        return Err(ApplicabilityError);
    }
    self.instructions.iter().try_for_each(|value| match value {
        Instruction::CopyAbility
        | Instruction::OfferDynamicManaPayment { .. }
        | Instruction::PayLife { .. }
        | Instruction::PayEnergy { .. }
        | Instruction::ExileFromZone { .. }
        | Instruction::TapOrUntapControlled { .. }
        | Instruction::ExertAbilitySource
        | Instruction::CollectEvidence { .. }
        | Instruction::Blight { .. }
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
        | Instruction::AttachPermanent { .. }
        | Instruction::SkipNextUntap { .. }
        | Instruction::RequireCombatAction { .. }
        | Instruction::CreateTemporaryRule { .. }
        | Instruction::CreateTokenCopy { .. }
        | Instruction::CreateAuraTokenAttached { .. }
        | Instruction::ExploreSubject(_)
        | Instruction::ScheduleDelayedEventTrigger { .. }
        | Instruction::CounterSpellToExile
        | Instruction::UntapControlledCreatures
        | Instruction::SetGroupTapped { .. }
        | Instruction::TapAttackersAndSkipNextUntap
        | Instruction::FightSourceWithTarget
        | Instruction::ClearSourceCounters(_)
        | Instruction::TransformSource
        | Instruction::TransformSourceAndAttachToTarget
        | Instruction::AdvanceClassLevel(_)
        | Instruction::SolveSourceCase
        | Instruction::BecomePreparedSource
        | Instruction::TransformSubtype(_)
        | Instruction::ModifyAnyNumberOfPermanents { .. }
        | Instruction::WeakenTargetByChosenX
        | Instruction::BoostTargetPower { .. }
        | Instruction::OfferSourceCounters { .. }
        | Instruction::OptionalSequence { .. }
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
        | Instruction::SetBlockDeclarations(_)
        | Instruction::ReselectAttackDefender
        | Instruction::ChooseAttackDirection
        | Instruction::AdditionalLoyaltyActivation
        | Instruction::MoveLoyaltyCounterToEnteringPermanent
        | Instruction::Ninjutsu
        | Instruction::Unearth
        | Instruction::Cascade
        | Instruction::FlipCoin { .. }
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
        | Instruction::PutQuestCounterOnSource
        | Instruction::PutStorageCounterOnSource
        | Instruction::AbilitySourceControllerLosesLife(_)
        | Instruction::RelatedPlayerLosesLife(_)
        | Instruction::LinkedPlayerLosesDamageAmount
        | Instruction::RelatedPlayerLosesHalfLife
        | Instruction::UntapAbilitySource
        | Instruction::TapAbilitySource
        | Instruction::RegenerateSource
        | Instruction::RegenerateControlledSubtype(_)
        | Instruction::RegenerateTarget
        | Instruction::DestroyGroupWithoutRegeneration(_)
        | Instruction::GoadTarget
        | Instruction::RequireTargetAttack
        | Instruction::RemoveTargetFromCombat
        | Instruction::GoadOtherPlayersCreatures
        | Instruction::SetControlledCreatureSizeToHand
        | Instruction::ModifyGroupByAmount { .. }
        | Instruction::DamageFromPower(_)
        | Instruction::CreatureDealsPowerDamageToOther { .. }
        | Instruction::DamageAmount(_)
        | Instruction::DamageAmountThenExileIfDies(_)
        | Instruction::DamageCreatureAtSpellMastery { .. }
        | Instruction::DrainOpponents(_)
        | Instruction::ExtortDrain
        | Instruction::ResolveGrowth(_)
        | Instruction::ResolveCounterKeyword(_)
        | Instruction::SacrificeEvoked
        | Instruction::SacrificeStateTriggerSource
        | Instruction::SacrificeUpkeepSource(_)
        | Instruction::CounterWardSource
        | Instruction::ResolveKeywordTrigger(_)
        | Instruction::GrantProtectionChosenColor(_)
        | Instruction::PutIndestructibleCounter
        | Instruction::PutLoyaltyCounter
        | Instruction::AnimateTargetAsCountedCreature { .. }
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
        | Instruction::WatchTargetCombatDamageDraw
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
        | Instruction::DivideTargetPermanentsAndSacrificePile
        | Instruction::DrawToChosenOpponentHandSize
        | Instruction::LookTopPlaneswalkers
        | Instruction::RevealTopPermanentOrHand
        | Instruction::LookTopLandBattlefieldTapped
        | Instruction::LookTopReveal { .. }
        | Instruction::LookTopTakeOne { .. }
        | Instruction::ReturnChosenXGraveyardCreatures
        | Instruction::ReturnGraveyardTargetsTapped(_)
        | Instruction::ReturnCapturedDyingCreature
        | Instruction::ReturnCapturedDyingCreatureWithTypeAndColor { .. }
        | Instruction::ReturnGraveyardCreatureWithSubtypeCounters { .. }
        | Instruction::ReturnGraveyardCohort(_)
        | Instruction::DivideThreeDamageAmongTargets
        | Instruction::DistributePlusOneCounters { .. }
        | Instruction::DamageOrderedTargets { .. }
        | Instruction::CopyOtherTargetAsArtifact
        | Instruction::ExileUntilOtherTargetLeaves
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
        | Instruction::SearchLibrary { .. }
        | Instruction::Adapt(_)
        | Instruction::BoostSourceByCastManaValue
        | Instruction::BoostLoneAttacker
        | Instruction::CreateTokenAndAttach { .. }
        | Instruction::BoostSaddledAttacker { .. }
        | Instruction::AddExperienceCounter
        | Instruction::ControlManaSourceThroughNextTurn
        | Instruction::GrantControllerFlashUntilCleanup
        | Instruction::RequireOpponentsCreaturesBlockThisTurn
        | Instruction::RequireBlocksAgainstTargetThisTurn
        | Instruction::SuppressGroupKeywords { .. }
        | Instruction::ModifyGroupUntilEndOfTurn { .. }
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
        | Instruction::PhaseOut(_)
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
        | Instruction::CreateEmblem(_)
        | Instruction::CreateOpponentEmblems(_)
        | Instruction::DrawAndLoseLifeForTargetGraveyardCreatures
        | Instruction::DrawHighestEntrantController
        | Instruction::DrawRelatedPlayer
        | Instruction::DrawCombatDamageSourceController
        | Instruction::DrawControllerAndTarget { .. }
        | Instruction::DrawEachPlayer { .. }
        | Instruction::DrawPlayersControllingName(_)
        | Instruction::DrawTargetPlayer { .. }
        | Instruction::DrawPlayer { .. }
        | Instruction::ReturnSourceGraveyardToBattlefield { .. }
        | Instruction::ReturnUpToOneRecentGraveyardCard
        | Instruction::ReturnSourceGraveyardToHand
        | Instruction::ExileUpToTwoGraveyardTargetsGainLife
        | Instruction::ReturnTargetGraveyardCreatureToHand
        | Instruction::ReturnRandomGraveyardCards { .. }
        | Instruction::ChooseRevealedHandCard { .. }
        | Instruction::ReturnUpToOneOwnGraveyardCreatureToHand
        | Instruction::ReturnOwnedCommandersToHand
        | Instruction::ExileTopForFreePlay
        | Instruction::ExileTargetUntilNonlandForFreeCast
        | Instruction::ExileTopRepeatOnPermanentEntry
        | Instruction::ShuffleEachHandAndGraveyard
        | Instruction::EndControllerTurn
        | Instruction::ShuffleOwnLibrary
        | Instruction::ShuffleOwnGraveyardIntoLibrary
        | Instruction::ShuffleTargetGraveyardCards { .. }
        | Instruction::ExileCreaturesAndPlaneswalkersBelowRevealedManaValue
        | Instruction::CastLinkedSpellMatchingX
        | Instruction::DestroyDamagedPlayersNonlandsAtX
        | Instruction::PutChosenXChargeCounters
        | Instruction::DestroyNonlandsAtSourceChargeCount
        | Instruction::AddCounterAndDischargeOnResolution(_)
        | Instruction::ExileAllGraveyards
        | Instruction::ExileCreatureAtMostRevealedPower
        | Instruction::ExileTarget
        | Instruction::ImprintTargetLand
        | Instruction::ImprintTargetCreature
        | Instruction::ExileTargetAsAbilityDonor
        | Instruction::ExileTargetUntilSourceLeaves
        | Instruction::ExileUntilNextEndStep
        | Instruction::ExileTargetUntilOpponentMonarch
        | Instruction::RememberPermanentTarget
        | Instruction::ControllerLosesRememberedManaValue
        | Instruction::ExiledTargetControllerGainsPower
        | Instruction::RepeatOpponentLifeLoss
        | Instruction::OpponentsLoseAndControllerGains(_)
        | Instruction::AdjustEachOpponentLife { .. }
        | Instruction::AdjustPlayersLife { .. }
        | Instruction::GainControlOfSpell
        | Instruction::ChangeOnlyTarget
        | Instruction::LoseLifeEqualToTargetSpellManaValue
        | Instruction::ChooseNewTargets
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
        | Instruction::DestroyGroup(_)
        | Instruction::DoubleCountersOnTargets
        | Instruction::ExileCombatDamagedCreature
        | Instruction::DestroyCapturedDamageSource
        | Instruction::RemoveCreatureCounters { .. }
        | Instruction::UntapControlledLands
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
        | Instruction::DamageTargetControllersCreatures { .. }
        | Instruction::DrawAndDamageOpponents { .. }
        | Instruction::BoostSource { .. }
        | Instruction::ResolvePowerAbility(_)
        | Instruction::PutCountersOnSource { .. }
        | Instruction::UnattachEquipmentAndProtectHost
        | Instruction::Unattach
        | Instruction::Attach(_)
        | Instruction::DestroyTarget
        | Instruction::DestroyTargetWithoutRegeneration
        | Instruction::ReturnOwnGraveyardCreatureToBattlefield
        | Instruction::ReturnAnyGraveyardCreatureBlackSubtype(_)
        | Instruction::ReturnAnyGraveyardCreatureOrPlaneswalker
        | Instruction::ReturnOwnGraveyardPermanentToBattlefield { .. }
        | Instruction::Scry { .. }
        | Instruction::Surveil { .. }
        | Instruction::Mill { .. }
        | Instruction::AffectPermanentTargets { .. }
        | Instruction::MillEachPlayer(_)
        | Instruction::PutLibraryCohortIntoGraveyard(_)
        | Instruction::ExileOnePerPlayerUntilSourceLeaves
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
        | Instruction::Discard { .. } => Err(ApplicabilityError),
        Instruction::ReturnToOwnersHand
            if self.target == spellwake_engine::program::BodyTarget::SingleOwnGraveyardCard =>
        {
            Err(ApplicabilityError)
        }
        Instruction::CreatePreventionShield { .. }
            if matches!(
                self.target,
                spellwake_engine::program::BodyTarget::SinglePlayer
                    | spellwake_engine::program::BodyTarget::SingleCreatureOrPlayer
            ) =>
        {
            Err(ApplicabilityError)
        }
        Instruction::CreateTokensToFight(_)
        | Instruction::FightPairedCreatures(_)
        | Instruction::OfferOpponentsDamage
        | Instruction::TemptWithTokens { .. }
        | Instruction::ChooseTypeThenDestroyUnchosen
        | Instruction::OrderArtifactCreatureLandSacrifices
        | Instruction::ChooseCreatureTypeForPlayer { .. } => Err(ApplicabilityError),
        Instruction::CreateTokens { creations }
        | Instruction::CreateAttackingTokens { creations } => creations
            .iter()
            .try_for_each(|creation| creation.definition.program().check_current10()),
        Instruction::CreateContinuousEffect { parts, .. } => parts.0.iter().try_for_each(|part| {
            if matches!(part, ResolvedPartSpec::BlockingRestriction(_)) {
                return Err(ApplicabilityError);
            }
            if let ResolvedPartSpec::Abilities(change) = part {
                change.check_current10()?;
            }
            Ok(())
        }),
        Instruction::BranchIfFalse { condition, .. }
            if [&condition.left, &condition.right].iter().any(|value| {
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
            Err(ApplicabilityError)
        }
        Instruction::BranchIfFalse { .. }
        | Instruction::Jump { .. }
        | Instruction::OptionalEffect { .. }
        | Instruction::OfferManaPayment { .. }
        | Instruction::GainControllerLife { .. }
        | Instruction::DrainTargetAndGain(_)
        | Instruction::AdjustControllerLife { .. }
        | Instruction::ExchangeLifeWithSourceToughness
        | Instruction::AdjustTargetLife { .. }
        | Instruction::ChooseCreatureType { .. }
        | Instruction::Damage { .. }
        | Instruction::CreatePreventionShield { .. }
        | Instruction::CreateReflectingShield
        | Instruction::CreateDamageMultiplier { .. }
        | Instruction::ReturnToOwnersHand
        | Instruction::CounterSpell
        | Instruction::CopySpell
        | Instruction::CopyTargetToSource { .. }
        | Instruction::PutPowerToughnessCounters { .. }
        | Instruction::PutNamedCounterOnTarget { .. }
        | Instruction::AddPlayerCounter { .. }
        | Instruction::DiscardFiltered { .. }
        | Instruction::ExileTopPlay { .. }
        | Instruction::LookTopChoose { .. }
        | Instruction::MoveGraveyardGroup { .. }
        | Instruction::PutHandPermanentFiltered { .. }
        | Instruction::RepeatLoop { .. }
        | Instruction::RevealUntilMatch { .. }
        | Instruction::ShufflePlayerLibrary(_)
        | Instruction::ShuffleTargetIntoLibrary
        | Instruction::RevealPlayerHand(_)
        | Instruction::ExilePlayerGraveyards(_)
        | Instruction::PayControllerEnergy(_)
        | Instruction::RemoveCountersFromPermanent { .. }
        | Instruction::DamageGroupAmount { .. }
        | Instruction::DoublePowerToughness { .. }
        | Instruction::DisallowDamagePreventionThisTurn
        | Instruction::GrantNoMaxHandSizeRestOfGame
        | Instruction::DestroySubject { .. }
        | Instruction::GrantCardPlayPermission { .. }
        | Instruction::DrawController { .. }
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
        | Instruction::CounterSpellToLibrary { .. }
        | Instruction::CounterTriggeringStackObject
        | Instruction::SetSubjectTapped { .. } => Ok(()),
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
        | ResolvedPart::CompoundAbilities(_) => Err(ApplicabilityError),
        ResolvedPart::Abilities(change) => change.check_current10(),
        ResolvedPart::Control(_)
        | ResolvedPart::Text(_)
        | ResolvedPart::Names(_)
        | ResolvedPart::Type(_)
        | ResolvedPart::Color(_)
        | ResolvedPart::SetPT(_)
        | ResolvedPart::SetToughness(_)
        | ResolvedPart::ModifyPT(_)
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
            | PendingWorkKind::ImpulseReplacement
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
        | RuleFrame::PlaneswalkerType(_)
        | RuleFrame::AttackDirection(_)
        | RuleFrame::DrawReplacement(_)
        | RuleFrame::DamageRecipientChoice(_)
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
        RuleFrame::LoyaltyRemoval(_)
        | RuleFrame::ResolutionManaColor(_)
        | RuleFrame::OptionIndex(_)
        | RuleFrame::Proliferate(_)
        | RuleFrame::CounterRemoval(_)
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
        | EffectDecision::PlaneswalkerType(_)
        | EffectDecision::AttackDirection(_)
        | EffectDecision::DrawReplacement(_)
        | EffectDecision::DamageRecipientChoice(_)
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
        EffectDecision::LoyaltyRemoval(_)
        | EffectDecision::ResolutionManaColor(_)
        | EffectDecision::OptionIndex(_)
        | EffectDecision::Proliferate(_)
        | EffectDecision::CounterRemoval(_)
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
        DecisionReply::TransformOrder(_) => Err(ApplicabilityError),
        DecisionReply::ChooseLandKind(_)
        | DecisionReply::MutateOnTop(_)
        | DecisionReply::ChoosePile(_)
        | DecisionReply::PlaneswalkerType(_)
        | DecisionReply::AttackDirection(_)
        | DecisionReply::DrawReplacement(_)
        | DecisionReply::ChooseDamageRecipient(_)
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
        | DecisionReply::RemoveCounters(_)
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
        | RulesOccurrence::BecameMonstrous { .. } => Ok(()),
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
