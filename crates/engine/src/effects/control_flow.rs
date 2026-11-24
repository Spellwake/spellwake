//! Forward control instructions use the existing payload/cursor/journal owners.
use super::*;
use crate::program::ResolutionCondition;

impl EffectWindow {
    /// Exact installed trigger-count context, shared by amounts and conditions.
    #[cfg(test)]
    pub(super) fn resolution_trigger_count(
        &self,
        controller: SeatId,
    ) -> Result<RulesInteger, EffectError> {
        self.resolution_trigger_count_at(controller, self.resolution_cursor())
    }
    pub(super) fn resolution_trigger_count_at(
        &self,
        controller: SeatId,
        cursor: Option<&ResolutionCursor>,
    ) -> Result<RulesInteger, EffectError> {
        let entry = self
            .window
            .resolving()
            .ok_or(EffectError::InconsistentProgram)?;
        let cursor = cursor.ok_or(EffectError::InconsistentProgram)?;
        if cursor.entry != entry.id() || entry.controller() != controller {
            return Err(EffectError::InconsistentProgram);
        }
        let binding = self
            .execution_bindings(entry.id())
            .and_then(|value| value.trigger.as_ref())
            .ok_or(EffectError::InconsistentProgram)?;
        if !matches!(entry.kind(), StackKind::TriggeredAbility { source } if source == &binding.source.object)
        {
            return Err(EffectError::InconsistentProgram);
        }
        Ok(RulesInteger::from_usize(binding.causes.len()))
    }
    /// Move the sole live cursor; no instruction can jump backwards or past end.
    pub(super) fn set_instruction(&mut self, destination: usize) -> Result<(), EffectError> {
        let cursor = self
            .resolution_cursor()
            .ok_or(EffectError::WrongResolutionFrame)?;
        let entry = self
            .window
            .resolving()
            .ok_or(EffectError::WrongResolutionFrame)?;
        let length = self
            .payloads
            .get(&cursor.entry)
            .and_then(StackPayload::instructions)
            .ok_or(EffectError::InconsistentProgram)?
            .len();
        if cursor.entry != entry.id() || destination <= cursor.instruction || destination > length {
            return Err(EffectError::InconsistentProgram);
        }
        self.resolution_cursor_mut()
            .ok_or(EffectError::WrongResolutionFrame)?
            .instruction = destination;
        Ok(())
    }
    pub(super) fn progress_control(
        &mut self,
        entry: &crate::stack_window::StackEntry,
        instruction: &Instruction,
        append: &mut RulesJournal,
    ) -> Result<bool, EffectError> {
        let from = self
            .instruction_index()
            .ok_or(EffectError::WrongResolutionFrame)?;
        if matches!(
            instruction,
            Instruction::BranchIfFalse { .. }
                | Instruction::Jump { .. }
                | Instruction::RepeatLoop { .. }
        ) && (self.window.resolving() != Some(entry)
            || self.resolution_cursor().map(|cursor| cursor.entry) != Some(entry.id()))
        {
            return Err(EffectError::InconsistentProgram);
        }
        match instruction {
            Instruction::BranchIfFalse {
                condition,
                destination,
            } => {
                let ResolutionCondition {
                    left,
                    comparison,
                    right,
                } = condition;
                let left = self.resolve_value_expr(entry.controller(), left)?;
                let right = self.resolve_value_expr(entry.controller(), right)?;
                let result = comparison.evaluate(&left, &right);
                let next = if result {
                    from.checked_add(1)
                        .ok_or(EffectError::InconsistentProgram)?
                } else {
                    *destination
                };
                self.set_instruction(next)?;
                append
                    .records
                    .push(crate::occurrence::RulesRecord::InstructionBranch {
                        entry: entry.id(),
                        instruction: from,
                        condition: condition.clone(),
                        left,
                        right,
                        result,
                        next,
                    });
            }
            Instruction::Jump { destination } => {
                self.set_instruction(*destination)?;
                append
                    .records
                    .push(crate::occurrence::RulesRecord::InstructionJump {
                        entry: entry.id(),
                        instruction: from,
                        next: *destination,
                    });
            }
            Instruction::RepeatLoop { count, body } => {
                let iterations = self
                    .resolve_value_expr(entry.controller(), count)?
                    .max(0.into())
                    .min(RulesInteger::from_usize(256))
                    .to_usize()
                    .ok_or(EffectError::InconsistentProgram)?;
                let body_len = body.instructions.len();
                let inserted = iterations.saturating_mul(body_len);
                let mut unrolled = Vec::with_capacity(inserted);
                for k in 0..iterations {
                    let base = isize::try_from(from + k * body_len)
                        .map_err(|_| EffectError::InconsistentProgram)?;
                    for op in &body.instructions {
                        let mut cloned = op.clone();
                        cloned.shift_jumps(base, 0);
                        unrolled.push(cloned);
                    }
                }
                let Some(StackPayload::Instructions(payload)) = self.payloads.get_mut(&entry.id())
                else {
                    return Err(EffectError::InconsistentProgram);
                };
                let delta =
                    isize::try_from(inserted).map_err(|_| EffectError::InconsistentProgram)? - 1;
                for op in &mut payload.body.instructions {
                    op.shift_jumps(delta, from + 1);
                }
                payload.body.instructions.splice(from..=from, unrolled);
                for mode in &mut payload.modes {
                    mode.shift_for_splice(from, inserted);
                }
            }
            Instruction::OptionalSequence { .. }
            | Instruction::OptionalEffect { .. }
            | Instruction::OfferManaPayment { .. }
            | Instruction::OfferDynamicManaPayment { .. }
            | Instruction::PayLife { .. }
            | Instruction::PayEnergy { .. }
            | Instruction::ExileFromZone { .. }
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
            | Instruction::CreateAuraTokenAttached { .. }
            | Instruction::ExploreSubject(_)
            | Instruction::ScheduleDelayedEventTrigger { .. }
            | Instruction::TapOrUntapControlled { .. }
            | Instruction::ExertAbilitySource
            | Instruction::CollectEvidence { .. }
            | Instruction::Blight { .. }
            | Instruction::ModifyAnyNumberOfPermanents { .. }
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
            | Instruction::Discard { .. }
            | Instruction::DiscardFiltered { .. }
            | Instruction::BoostSaddledAttacker { .. }
            | Instruction::SetControlledCreatureSizeToHand
            | Instruction::ModifyGroupByAmount { .. }
            | Instruction::AddExperienceCounter
            | Instruction::AddPlayerCounter { .. }
            | Instruction::ExileTopPlay { .. }
            | Instruction::LookTopChoose { .. }
            | Instruction::MoveGraveyardGroup { .. }
            | Instruction::PutHandPermanentFiltered { .. }
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
            | Instruction::PutTargetInLibrary(_)
            | Instruction::PutOwnHandCardOnLibraryBottom
            | Instruction::PutGroupOnLibraryBottom(_)
            | Instruction::ExileGraveyardTarget
            | Instruction::ExileCapturedGraveyardCard
            | Instruction::GrantGraveyardCastAndWatchAbilities
            | Instruction::GainCastCardAbilities { .. }
            | Instruction::ExileCardsFromGraveyard { .. }
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
            | Instruction::RegenerateTarget
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
            | Instruction::DamageTargetControllersCreatures { .. }
            | Instruction::DrawAndDamageOpponents { .. }
            | Instruction::RepeatOpponentLifeLoss
            | Instruction::OpponentsLoseAndControllerGains(_)
            | Instruction::AdjustEachOpponentLife { .. }
            | Instruction::AdjustPlayersLife { .. }
            | Instruction::CreatePreventionShield { .. }
            | Instruction::CreateReflectingShield
            | Instruction::CreateDamageMultiplier { .. }
            | Instruction::OfferSourceCounters { .. }
            | Instruction::ResolveKeywordTrigger(_)
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
            | Instruction::SearchLibrary { .. }
            | Instruction::IncreaseSpeed
            | Instruction::BecomeMonarch(_)
            | Instruction::TapEnchantedCreature
            | Instruction::RequireCondition(_)
            | Instruction::PayControllerLife(_)
            | Instruction::PayControllerEnergy(_)
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
            | Instruction::DamageGroupAmount { .. }
            | Instruction::DoublePowerToughness { .. }
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
            | Instruction::BoostSourceByCastManaValue
            | Instruction::BoostLoneAttacker
            | Instruction::Attach(_)
            | Instruction::GoadTarget
            | Instruction::RequireTargetAttack
            | Instruction::RemoveTargetFromCombat
            | Instruction::DestroyTarget
            | Instruction::DestroyTargetWithoutRegeneration
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
            | Instruction::RemoveCountersFromPermanent { .. }
            | Instruction::ExileCreatureAtMostRevealedPower
            | Instruction::ExileTarget
            | Instruction::ImprintTargetLand
            | Instruction::ImprintTargetCreature
            | Instruction::ExileTargetAsAbilityDonor
            | Instruction::ExileTargetUntilSourceLeaves
            | Instruction::ExileUntilNextEndStep
            | Instruction::ExileTargetUntilOpponentMonarch
            | Instruction::ReturnToOwnersHand
            | Instruction::ReturnOwnGraveyardCreatureToBattlefield
            | Instruction::ReturnAnyGraveyardCreatureBlackSubtype(_)
            | Instruction::ReturnAnyGraveyardCreatureOrPlaneswalker
            | Instruction::ReturnOwnGraveyardPermanentToBattlefield { .. }
            | Instruction::CounterSpellToExile
            | Instruction::CounterSpellToLibrary { .. }
            | Instruction::CounterSpell
            | Instruction::GainControlOfSpell
            | Instruction::ChangeOnlyTarget
            | Instruction::LoseLifeEqualToTargetSpellManaValue
            | Instruction::ChooseNewTargets
            | Instruction::CopyAbility
            | Instruction::CopySpell
            | Instruction::CopyTargetToSource { .. }
            | Instruction::CreateContinuousEffect { .. }
            | Instruction::WeakenTargetByChosenX
            | Instruction::BoostTargetPower { .. }
            | Instruction::AnimateTargetAsCountedCreature { .. }
            | Instruction::GrantProtectionChosenColor(_)
            | Instruction::PutIndestructibleCounter
            | Instruction::PutLoyaltyCounter
            | Instruction::PutNamedCounterOnTarget { .. }
            | Instruction::PutPowerToughnessCounters { .. }
            | Instruction::Scry { .. }
            | Instruction::Surveil { .. }
            | Instruction::Mill { .. }
            | Instruction::DrawTargetPlayer { .. }
            | Instruction::DrawPlayer { .. }
            | Instruction::DrawHighestEntrantController
            | Instruction::DrawRelatedPlayer
            | Instruction::DrawCombatDamageSourceController
            | Instruction::DrawAndLoseLifeForTargetGraveyardCreatures
            | Instruction::DrawControllerAndTarget { .. }
            | Instruction::DrawEachPlayer { .. }
            | Instruction::DrawPlayersControllingName(_)
            | Instruction::DrawController { .. } => return Ok(false),
        }
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        game::{GameCommand, GameContext, GameId, GameInput, GameState},
        mana::ManaCost,
        object_identity::CardId,
        program::{
            Comparison, CreatureTypeSlot, Program, TriggerGrouping, TriggerSpec, TriggeredAbility,
            TriggeredAbilityId,
        },
        test_support::creature_types,
        turn::{Step, TurnStart},
    };
    use std::{collections::BTreeSet, sync::Arc};
    const A: SeatId = SeatId(0);
    const B: SeatId = SeatId(1);
    fn paused() -> EffectWindow {
        paused_with_tail(None)
    }
    fn paused_with_tail(tail: Option<Instruction>) -> EffectWindow {
        let mut program = Program::instant(
            ManaCost::free(),
            ExecutableBody::new(
                BodyTarget::None,
                vec![Instruction::AdjustControllerLife {
                    adjustment: crate::life::LifeAdjustment::Gain(0.into()),
                }],
            ),
        );
        program.triggered.push(TriggeredAbility {
            id: TriggeredAbilityId(1),
            condition: TriggerSpec::self_cast(),
            grouping: TriggerGrouping::EachOccurrence,
            body: ExecutableBody::new(
                BodyTarget::None,
                vec![
                    Instruction::ChooseCreatureType {
                        into: CreatureTypeSlot::new(0),
                        excluding: BTreeSet::new(),
                    },
                    Instruction::BranchIfFalse {
                        condition: ResolutionCondition {
                            left: crate::program::ValueExpr::event(
                                crate::program::EventValueKind::MatchedOccurrences,
                            ),
                            comparison: Comparison::Equal,
                            right: crate::program::ValueExpr::Literal(1.into()),
                        },
                        destination: 2,
                    },
                ],
            ),
        });
        if let Some(tail) = tail {
            program.triggered[0].body.instructions.push(tail);
        }
        let seed = GameSeed::with_uniform_life(
            vec![A, B],
            A,
            vec![
                SeedCard {
                    card: CardId(1),
                    owner: A,
                    destination: ObjectDestination::Hand,
                    program,
                },
                SeedCard {
                    card: CardId(2),
                    owner: B,
                    destination: ObjectDestination::Battlefield { controller: B },
                    program: Program::artifact(None, vec![]),
                },
            ],
            vec![CardId(2)],
            vec![],
            20.into(),
            CombatRules::UnrestrictedFreeForAll,
            creature_types::selected_vocabulary().unwrap(),
            Arc::new(spellwake_contracts::names::NameRules::literal_names()),
        );
        let context = GameContext::new(
            "conditional-context-unit".into(),
            seed.creature_vocabulary.rules_revision().into(),
            "literal partial".into(),
            "unit".into(),
            "unit".into(),
            seed.creature_vocabulary.id(),
            seed.name_rules.id(),
        )
        .unwrap();
        let mut game = GameState::initialize_scheduled(
            GameId(6051),
            context,
            seed,
            TurnStart {
                number: 3,
                step: Step::PrecombatMain,
            },
        )
        .unwrap()
        .into_state();
        let source = game
            .execution()
            .window()
            .cards()
            .current_identity(CardId(1))
            .unwrap();
        for (actor, command) in [
            (
                A,
                GameCommand::CastUntargeted {
                    spell: source,
                    payment: None,
                },
            ),
            (A, GameCommand::Pass),
            (B, GameCommand::Pass),
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
        assert_eq!(game.execution().instruction_index(), Some(0));
        game.execution().clone()
    }
    #[test]
    fn count_context_rejects_wrong_controller_cursor_missing_binding_and_source() {
        let state = paused();
        assert_eq!(state.resolution_trigger_count(A), Ok(1.into()));
        assert_eq!(
            state.resolution_trigger_count(B),
            Err(EffectError::InconsistentProgram)
        );
        let resolving = state.window.resolving().unwrap().id();
        let original = state.window.entries()[0].id();
        assert_ne!(original, resolving);
        let mut wrong = state.clone();
        wrong.resolution_cursor_mut().unwrap().entry = original;
        let saved = wrong.clone();
        assert_eq!(
            wrong.resolution_trigger_count(A),
            Err(EffectError::InconsistentProgram)
        );
        assert_eq!(wrong, saved);
        let mut missing = state.clone();
        let StackPayload::Instructions(payload) = missing.payloads.get_mut(&resolving).unwrap()
        else {
            panic!("instruction payload")
        };
        payload.bindings.trigger = None;
        assert_eq!(
            missing.resolution_trigger_count(A),
            Err(EffectError::InconsistentProgram)
        );
        let other = state
            .window
            .cards()
            .get(state.window.cards().current_identity(CardId(2)).unwrap())
            .unwrap()
            .clone();
        let mut foreign = state;
        let StackPayload::Instructions(payload) = foreign.payloads.get_mut(&resolving).unwrap()
        else {
            panic!("instruction payload")
        };
        payload.bindings.trigger.as_mut().unwrap().source.object = other;
        assert_eq!(
            foreign.resolution_trigger_count(A),
            Err(EffectError::InconsistentProgram)
        );
    }
    #[test]
    fn checked_control_rejects_foreign_entry_and_invalid_destinations_before_changes() {
        let state = paused();
        let foreign = state.window.entries()[0].clone();
        let actual = state.window.resolving().unwrap().clone();
        for (entry, op) in [
            (&foreign, Instruction::Jump { destination: 1 }),
            (&actual, Instruction::Jump { destination: 0 }),
            (
                &actual,
                Instruction::Jump {
                    destination: usize::MAX,
                },
            ),
        ] {
            let mut next = state.clone();
            let mut append = RulesJournal::default();
            assert!(next.progress_control(entry, &op, &mut append).is_err());
            assert_eq!(next, state);
            assert!(append.records.is_empty());
        }
    }
    #[test]
    fn exhausted_allocation_after_accepted_local_and_branch_has_no_adoptable_partial_transition() {
        use crate::token::{TokenCreation, TokenDefinition, TokenName, TokenRecipients};
        let v = creature_types::selected_vocabulary().unwrap();
        let token = TokenDefinition::new(
            Program::creature(None, 1.into(), 1.into(), BTreeSet::new()),
            TokenName::Default {
                ordered_subtypes: vec![],
            },
            &v,
        )
        .unwrap();
        let healthy = paused_with_tail(Some(Instruction::CreateTokens {
            creations: vec![TokenCreation {
                definition: token,
                amount: ValueExpr::Literal(1.into()),
                base_size: None,
                enters_tapped: false,
                recipients: TokenRecipients::Controller,
            }],
        }));
        let human = v.resolve_label("Human").unwrap();
        let frame = EffectDecision::CreatureType(healthy.creature_type_frame().unwrap());
        let accepted = healthy
            .resume_rule(
                healthy.revision(),
                &frame,
                A,
                DecisionReply::CreatureType(human),
                None,
            )
            .unwrap();
        assert!(accepted.append().records.iter().any(|row| matches!(
            row,
            crate::occurrence::RulesRecord::InstructionBranch {
                instruction: 1,
                result: true,
                next: 2,
                ..
            }
        )));
        let mut exhausted = healthy;
        exhausted.window.set_test_next_object_identity(u64::MAX);
        let saved = exhausted.clone();
        let frame = EffectDecision::CreatureType(exhausted.creature_type_frame().unwrap());
        let result = exhausted.resume_rule(
            exhausted.revision(),
            &frame,
            A,
            DecisionReply::CreatureType(human),
            None,
        );
        assert!(result.is_err());
        assert_eq!(exhausted, saved);
        assert!(exhausted.resolution_state().unwrap().locals().is_empty());
        assert_eq!(exhausted.instruction_index(), Some(0));
    }
}
