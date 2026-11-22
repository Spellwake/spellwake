//! Atomic single-instruction execution cuts during stack resolution.
use super::*;

impl EffectWindow {
    #[inline(never)]
    pub(super) fn apply_instruction(
        &mut self,
        entry: crate::stack_window::StackEntry,
        instruction: Instruction,
        copy_choice: Option<(SeatId, Vec<TargetChoice>)>,
        journal: &mut DamageJournal,
        append: &mut RulesJournal,
    ) -> Result<(), EffectError> {
        use crate::occurrence::{EventCause, RulesOccurrence, ZoneReason};
        let index = self
            .instruction_index()
            .ok_or(EffectError::WrongResolutionFrame)?;
        let bound = self.instruction_target(&entry)?;
        let choice = bound.choice();
        let target = choice.target;
        let before = self.rules_snapshot(None)?;
        enum InstructionCut {
            Ready(RulesOccurrence),
            Exit {
                exit: crate::stack_window::StackExit,
                reason: ZoneReason,
            },
            GraveyardEntry {
                exit: crate::stack_window::StackExit,
                previous: ObjectId,
                object: ObjectId,
                counters: PowerToughnessCounters,
                effect: Option<ResolvedContinuousEffect>,
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
            Instruction::Attach(_) => {
                let (StackKind::ActivatedAbility { source }
                | StackKind::TriggeredAbility { source }) = entry.kind()
                else {
                    return Err(EffectError::InconsistentProgram);
                };
                let Some(occurrence) = self.attach_if_legal(entry.id(), source.id(), target)?
                else {
                    return Ok(());
                };
                InstructionCut::Ready(occurrence)
            }
            Instruction::AffectPermanentTargets { .. }
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
            | Instruction::Discard { .. } => {
                return Err(EffectError::InconsistentProgram);
            }
            Instruction::OptionalSequence { .. }
            | Instruction::OptionalEffect { .. }
            | Instruction::OfferManaPayment { .. }
            | Instruction::BranchIfFalse { .. }
            | Instruction::Jump { .. }
            | Instruction::DamageFromPower(_)
            | Instruction::CreatureDealsPowerDamageToOther { .. }
            | Instruction::DamageAmount(_)
            | Instruction::DamageAmountThenExileIfDies(_)
            | Instruction::DamageCreatureAtSpellMastery { .. }
            | Instruction::Damage { .. }
            | Instruction::RememberPermanentTarget
            | Instruction::ControllerLosesRememberedManaValue
            | Instruction::ExiledTargetControllerGainsPower
            | Instruction::RegenerateTarget
            | Instruction::GoadTarget
            | Instruction::RequireTargetAttack
            | Instruction::RemoveTargetFromCombat
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
            | Instruction::ExileCreaturesAndPlaneswalkersBelowRevealedManaValue
            | Instruction::CastLinkedSpellMatchingX
            | Instruction::DestroyDamagedPlayersNonlandsAtX
            | Instruction::PutChosenXChargeCounters
            | Instruction::DestroyNonlandsAtSourceChargeCount
            | Instruction::AddCounterAndDischargeOnResolution(_)
            | Instruction::ExileAllGraveyards
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
            | Instruction::BoostSaddledAttacker { .. }
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
            | Instruction::RemoveLoyaltyExileTopForPlay
            | Instruction::DamageDamagedPlayersCreatures
            | Instruction::DamageOpponentsAmount(_)
            | Instruction::DamageRelatedPlayerAmount(_)
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
            | Instruction::ResolveKeywordTrigger(_)
            | Instruction::BoostSourceByCastManaValue
            | Instruction::BoostLoneAttacker
            | Instruction::DrawTargetPlayer { .. }
            | Instruction::DrawHighestEntrantController
            | Instruction::DrawRelatedPlayer
            | Instruction::DrawCombatDamageSourceController
            | Instruction::DrawAndLoseLifeForTargetGraveyardCreatures
            | Instruction::DrawControllerAndTarget { .. }
            | Instruction::DrawEachPlayer { .. }
            | Instruction::DrawPlayersControllingName(_)
            | Instruction::DrawController { .. }
            | Instruction::GainControllerLife { .. }
            | Instruction::DrainTargetAndGain(_)
            | Instruction::AdjustControllerLife { .. }
            | Instruction::ExchangeLifeWithSourceToughness
            | Instruction::AdjustTargetLife { .. }
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
            | Instruction::ChooseTypeThenDestroyUnchosen
            | Instruction::OrderArtifactCreatureLandSacrifices
            | Instruction::ChooseCreatureTypeForPlayer { .. }
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
            Instruction::CreateReflectingShield => {
                let amount = self.resolve_value_expr(entry.controller(), &ValueExpr::chosen_x())?;
                if !amount.is_positive() {
                    return Ok(());
                }
                let traits = self
                    .stack_copiable_traits(entry.id())
                    .ok_or(EffectError::InconsistentProgram)?;
                let source = self.resolution_damage_source(&entry)?;
                let reflection = crate::damage::DamageReflection {
                    colors: self.source_colors_for_damage(&before.view().evaluated, &source),
                    source,
                    keywords: traits.keywords.clone(),
                    creature_types: traits.creature_subtypes.clone(),
                    is_creature: traits.card_types.contains(&CardType::Creature),
                    target,
                };
                let shield = self.damage_state.add_reflecting_shield(
                    entry.id(),
                    entry.controller(),
                    amount,
                    reflection,
                )?;
                let created = DamageEffectCreation::Prevention {
                    shield,
                    instruction: index,
                };
                journal.created.push(created.clone());
                InstructionCut::Ready(RulesOccurrence::DamageEffectCreated(created))
            }
            Instruction::CreatePreventionShield { amount } => {
                let protected = match target {
                    TargetRef::Object(id) => DamageEffectTarget::Permanent(id),
                    TargetRef::Player(seat) => DamageEffectTarget::Player(seat),
                    TargetRef::Stack(_) => return Err(EffectError::InconsistentProgram),
                };
                let shield = self
                    .damage_state
                    .add_shield(entry.id(), protected, amount)?;
                let created = DamageEffectCreation::Prevention {
                    shield,
                    instruction: index,
                };
                journal.created.push(created.clone());
                InstructionCut::Ready(RulesOccurrence::DamageEffectCreated(created))
            }
            Instruction::CounterSpell | Instruction::CounterSpellToExile => {
                let TargetRef::Stack(countered) = target else {
                    return Err(EffectError::InconsistentProgram);
                };
                let view = before.view();
                let target_type_set = match self
                    .window
                    .entries()
                    .iter()
                    .find(|candidate| candidate.id() == countered)
                    .map(|candidate| candidate.kind())
                {
                    Some(StackKind::CardSpell(id)) => view
                        .evaluated
                        .get(*id)
                        .map(|properties| &properties.current_types),
                    Some(StackKind::SpellCopy { .. }) => self.stack_copiable_card_types(countered),
                    _ => None,
                }
                .cloned()
                .unwrap_or_default();
                self.resolution_cursor_mut()
                    .ok_or(EffectError::WrongResolutionFrame)?
                    .results
                    .counter_target_types = target_type_set;
                if self.spell_cannot_be_countered(countered)? {
                    return Ok(());
                }
                if self
                    .window
                    .entries()
                    .iter()
                    .find(|candidate| candidate.id() == countered)
                    .is_some_and(|candidate| {
                        !matches!(
                            candidate.kind(),
                            StackKind::CardSpell(_) | StackKind::SpellCopy { .. }
                        )
                    })
                {
                    let (window, exit) = self.window.counter_captured_entry(countered)?;
                    if exit.is_some() {
                        return Err(EffectError::InconsistentProgram);
                    }
                    self.window = window;
                    self.payloads.remove(&countered);
                    self.emit_from(
                        EventCause::CounterSpell {
                            entry: entry.id(),
                            instruction: index,
                            target: countered,
                        },
                        &before,
                        None,
                        append,
                        |_, _, _| Ok(vec![RulesOccurrence::AbilityCountered { entry: countered }]),
                    )?;
                    return Ok(());
                }
                let controller = self
                    .window
                    .entries()
                    .iter()
                    .find(|spell| spell.id() == countered)
                    .ok_or(EffectError::InconsistentProgram)?
                    .controller();
                self.resolution_cursor_mut()
                    .ok_or(EffectError::WrongResolutionFrame)?
                    .results
                    .countered_spell_controller = Some((countered, controller));
                let (window, exit) = self.window.record_targeted_resolution_move_with_exit(
                    self.window.revision(),
                    choice.id,
                    if instruction == Instruction::CounterSpellToExile {
                        ObjectDestination::Exile
                    } else {
                        ObjectDestination::Graveyard
                    },
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
            Instruction::GainControlOfSpell => {
                let TargetRef::Stack(spell) = target else {
                    return Err(EffectError::InconsistentProgram);
                };
                let controller = entry.controller();
                let previous = self.window.gain_spell_control(spell, controller)?;
                if previous == controller {
                    return Ok(());
                }
                InstructionCut::Ready(RulesOccurrence::SpellControllerChanged {
                    entry: spell,
                    before: previous,
                    after: controller,
                })
            }
            Instruction::ChooseNewTargets | Instruction::ChangeOnlyTarget => {
                let (actor, changes) = copy_choice.ok_or(EffectError::CopyChoiceRequired)?;
                if actor != entry.controller() {
                    return Err(EffectError::WrongChooser);
                }
                let TargetRef::Stack(original) = target else {
                    return Err(EffectError::InconsistentProgram);
                };
                if changes.is_empty() {
                    return Ok(());
                }
                let subjects = |window: &crate::stack_window::StackWindow| -> Result<Vec<TargetChoice>, EffectError> {
                    Ok(window.entries().iter().find(|s| s.id() == original)
                        .ok_or(EffectError::InconsistentProgram)?.targets().targets().iter().map(|b| b.choice()).collect())
                };
                let before = subjects(&self.window)?;
                self.window.change_targets(original, changes)?;
                let after = subjects(&self.window)?;
                if before == after {
                    return Ok(());
                }
                InstructionCut::Ready(RulesOccurrence::StackTargetsChanged {
                    entry: original,
                    before,
                    after,
                })
            }
            Instruction::LoseLifeEqualToTargetSpellManaValue => {
                let TargetRef::Stack(target) = target else {
                    return Err(EffectError::InconsistentProgram);
                };
                let value = self
                    .entry_spell_mana_value(target)
                    .ok_or(EffectError::InconsistentProgram)?;
                self.adjust_life_at_cut(
                    entry.id(),
                    index,
                    entry.controller(),
                    &crate::life::LifeAdjustment::Lose(value),
                    append,
                )?;
                return Ok(());
            }
            Instruction::CopyAbility => {
                let (actor, changes) = copy_choice.ok_or(EffectError::CopyChoiceRequired)?;
                if actor != entry.controller() {
                    return Err(EffectError::WrongChooser);
                }
                let TargetRef::Stack(original) = target else {
                    return Err(EffectError::InconsistentProgram);
                };
                let source = self
                    .window
                    .entries()
                    .iter()
                    .find(|s| s.id() == original)
                    .ok_or(EffectError::InconsistentProgram)?
                    .clone();
                let payload = self
                    .payloads
                    .get(&original)
                    .cloned()
                    .ok_or(EffectError::InconsistentProgram)?;
                InstructionCut::Ready(self.create_ability_copy(&source, payload, actor, changes)?)
            }
            Instruction::CopySpell => {
                let (actor, selected) = copy_choice.ok_or(EffectError::CopyChoiceRequired)?;
                if actor != entry.controller() {
                    return Err(EffectError::WrongChooser);
                }
                let TargetRef::Stack(original) = target else {
                    return Err(EffectError::InconsistentProgram);
                };
                let snapshot = self.capture_copy_source(original, 0.into())?;
                InstructionCut::Ready(self.create_copy(&snapshot, actor, selected)?)
            }
            Instruction::AnimateTargetAsCountedCreature {
                amount,
                subtype,
                keywords,
                duration,
            } => {
                use crate::characteristics::{
                    AbilityChange, InstructionSubtypeOperand, PowerToughness, ResolvedPartSpec,
                    ResolvedPartsSpec, SubtypeOperation, TypeChangeSpec, TypeOperation,
                };
                let TargetRef::Object(target) = target else {
                    return Err(EffectError::InconsistentProgram);
                };
                let count = self.resolve_value_expr(entry.controller(), &amount)?;
                let parts = ResolvedPartsSpec(vec![
                    ResolvedPartSpec::Type(TypeChangeSpec {
                        land_subtypes: SubtypeOperation::Keep,
                        supertypes: TypeOperation::Keep,
                        types: TypeOperation::Add(BTreeSet::from([CardType::Creature])),
                        subtypes: SubtypeOperation::Add(InstructionSubtypeOperand::Literal(
                            BTreeSet::from([subtype]),
                        )),
                    }),
                    ResolvedPartSpec::Abilities(AbilityChange::grant_keywords(keywords)),
                    ResolvedPartSpec::SetPT(PowerToughness {
                        power: count.clone(),
                        toughness: count,
                    }),
                ]);
                let locals = self
                    .resolution_state()
                    .ok_or(EffectError::WrongResolutionFrame)?
                    .locals()
                    .clone();
                let owner = self
                    .window
                    .cards()
                    .get(target)
                    .ok_or(EffectError::InconsistentProgram)?
                    .owner();
                InstructionCut::Continuous(self.characteristics.add_effect(
                    entry.id(),
                    target,
                    parts.materialize(&locals, entry.controller(), owner)?,
                    duration,
                )?)
            }
            Instruction::WeakenTargetByChosenX => {
                use crate::characteristics::{
                    PowerToughness, ResolvedDuration, ResolvedPart, ResolvedParts,
                };
                let TargetRef::Object(target) = target else {
                    return Err(EffectError::InconsistentProgram);
                };
                let change = RulesInteger::from(0)
                    .minus(&self.resolve_value_expr(entry.controller(), &ValueExpr::chosen_x())?);
                InstructionCut::Continuous(self.characteristics.add_effect(
                    entry.id(),
                    target,
                    ResolvedParts(vec![ResolvedPart::ModifyPT(PowerToughness {
                        power: change.clone(),
                        toughness: change,
                    })]),
                    ResolvedDuration::UntilEndOfTurn,
                )?)
            }
            Instruction::BoostTargetPower {
                amount,
                also_toughness,
                keywords,
            } => {
                use crate::characteristics::{
                    AbilityChange, PowerToughness, ResolvedDuration, ResolvedPart, ResolvedParts,
                };
                let TargetRef::Object(target) = target else {
                    return Err(EffectError::InconsistentProgram);
                };
                let power = self.resolve_value_expr(entry.controller(), &amount)?;
                let toughness = if also_toughness {
                    power.clone()
                } else {
                    0.into()
                };
                InstructionCut::Continuous(self.characteristics.add_effect(
                    entry.id(),
                    target,
                    ResolvedParts(vec![
                        ResolvedPart::Abilities(AbilityChange::grant_keywords(keywords)),
                        ResolvedPart::ModifyPT(PowerToughness { power, toughness }),
                    ]),
                    ResolvedDuration::UntilEndOfTurn,
                )?)
            }
            Instruction::CreateContinuousEffect { parts, duration } => {
                let TargetRef::Object(target) = target else {
                    return Err(EffectError::InconsistentProgram);
                };
                let Some(card) = self
                    .window
                    .cards()
                    .get(target)
                    .filter(|card| card.zone() == ObjectZone::Battlefield)
                else {
                    return Ok(());
                };
                let owner = card.owner();
                let duration = match duration {
                    crate::characteristics::ResolvedDuration::WhileControllingAbilitySource => {
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
                                    && card.controller() == Some(controller)
                            })
                            .and_then(GameObject::control_epoch)
                        else {
                            return Ok(());
                        };
                        crate::characteristics::ResolvedDuration::WhileSourceControlled {
                            source,
                            controller,
                            epoch,
                        }
                    }
                    value => value,
                };
                let locals = self
                    .resolution_state()
                    .ok_or(EffectError::WrongResolutionFrame)?;
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
            Instruction::GrantProtectionChosenColor(_) => {
                return Err(EffectError::InconsistentProgram);
            }
            Instruction::PutIndestructibleCounter => {
                let TargetRef::Object(object) = target else {
                    return Err(EffectError::InconsistentProgram);
                };
                let kind = CounterKind::Indestructible;
                let previous = self.counters_of_kind(object, kind);
                let added =
                    permanent_counters::effect_counter_multiplier(&before.view().evaluated, object);
                let after = previous.plus(&added);
                self.characteristics
                    .indestructible_counters
                    .insert(object, after.clone());
                self.characteristics
                    .refresh_indestructible_counter_grant(entry.id(), object)?;
                InstructionCut::Ready(RulesOccurrence::PermanentCounterChanged {
                    object,
                    kind,
                    before: previous,
                    after,
                })
            }
            Instruction::PutLoyaltyCounter => {
                let TargetRef::Object(target) = target else {
                    return Err(EffectError::InconsistentProgram);
                };
                let old = self
                    .characteristics
                    .loyalty
                    .get(&target)
                    .cloned()
                    .unwrap_or_else(|| 0.into());
                let added =
                    permanent_counters::effect_counter_multiplier(&before.view().evaluated, target);
                let new = old.plus(&added);
                self.characteristics.loyalty.insert(target, new.clone());
                InstructionCut::Ready(RulesOccurrence::LoyaltyCountersChanged {
                    object: target,
                    before: old,
                    after: new,
                })
            }
            Instruction::PutPowerToughnessCounters { counters } => {
                let TargetRef::Object(target) = target else {
                    return Err(EffectError::InconsistentProgram);
                };
                let counters =
                    self.adjusted_pt_counters(&before.view().evaluated, target, counters, None)?;
                let previous = self.characteristics.counters(target);
                self.characteristics.place_counters(target, &counters)?;
                InstructionCut::Counters {
                    object: target,
                    before: previous,
                    added: counters,
                    after: self.characteristics.counters(target),
                }
            }
            Instruction::ReturnOwnGraveyardCreatureToBattlefield
            | Instruction::ReturnOwnGraveyardPermanentToBattlefield { .. }
            | Instruction::ReturnAnyGraveyardCreatureBlackSubtype(_)
            | Instruction::ReturnAnyGraveyardCreatureOrPlaneswalker => {
                let enters_tapped = matches!(
                    instruction,
                    Instruction::ReturnOwnGraveyardPermanentToBattlefield { tapped: true }
                );
                let (window, mut exit) = self.window.record_targeted_resolution_move_with_exit(
                    self.window.revision(),
                    choice.id,
                    ObjectDestination::Battlefield {
                        controller: entry.controller(),
                    },
                )?;
                self.window = window;
                let crate::stack_window::StackExit::Object(movement) = &mut exit else {
                    return Err(EffectError::InconsistentProgram);
                };
                let previous = movement.before.id();
                let object = movement.after.id();
                self.settled_entry_destinations.remove(&previous);
                self.stage_resolution_entry_choice(previous, object)?;
                self.characteristics.register_battlefield_entry(object)?;
                if let Some(host) = self.resolution_aura_choices.remove(&previous) {
                    self.characteristics.attach(object, host, true)?;
                }
                if enters_tapped {
                    self.window.enter_tapped(object)?;
                }
                let effect = if let Instruction::ReturnAnyGraveyardCreatureBlackSubtype(kind) =
                    instruction
                {
                    use crate::characteristics::{
                        Color, ColorOperation, ResolvedDuration, ResolvedPart, ResolvedParts,
                        SubtypeOperation, TypeChange, TypeOperation,
                    };
                    Some(self.characteristics.add_effect(
                        entry.id(),
                        object,
                        ResolvedParts(vec![
                            ResolvedPart::Type(TypeChange {
                                land_subtypes: SubtypeOperation::Keep,
                                supertypes: TypeOperation::Keep,
                                types: TypeOperation::Keep,
                                subtypes: SubtypeOperation::Add(BTreeSet::from([kind])),
                            }),
                            ResolvedPart::Color(ColorOperation::Set(BTreeSet::from([
                                Color::Black,
                            ]))),
                        ]),
                        ResolvedDuration::NoStatedDuration,
                    )?)
                } else {
                    None
                };
                let counters = self.apply_entry_counters(
                    &before,
                    object,
                    PowerToughnessCounters::default(),
                    0,
                )?;
                movement.after = self
                    .window
                    .cards()
                    .get(object)
                    .ok_or(EffectError::InconsistentProgram)?
                    .clone();
                InstructionCut::GraveyardEntry {
                    exit,
                    previous,
                    object,
                    counters,
                    effect,
                }
            }
            Instruction::ExileCreatureAtMostRevealedPower => {
                return Err(EffectError::InconsistentProgram);
            }
            Instruction::DestroyTarget
            | Instruction::ExileTarget
            | Instruction::ImprintTargetLand
            | Instruction::ImprintTargetCreature
            | Instruction::ExileTargetAsAbilityDonor
            | Instruction::ExileTargetUntilSourceLeaves
            | Instruction::ExileUntilNextEndStep
            | Instruction::ExileTargetUntilOpponentMonarch
            | Instruction::ReturnToOwnersHand => {
                let linked_source = if instruction == Instruction::ExileTargetUntilSourceLeaves {
                    let StackKind::TriggeredAbility { source } = entry.kind() else {
                        return Err(EffectError::InconsistentProgram);
                    };
                    let source = source.id();
                    if self
                        .window
                        .cards()
                        .get(source)
                        .is_none_or(|object| object.zone() != ObjectZone::Battlefield)
                    {
                        return Ok(());
                    }
                    Some(source)
                } else {
                    None
                };
                let (destination, reason) = match instruction {
                    Instruction::DestroyTarget => {
                        (ObjectDestination::Graveyard, ZoneReason::Destroyed)
                    }
                    Instruction::ExileTarget
                    | Instruction::ImprintTargetLand
                    | Instruction::ImprintTargetCreature
                    | Instruction::ExileTargetAsAbilityDonor
                    | Instruction::ExileTargetUntilSourceLeaves
                    | Instruction::ExileUntilNextEndStep
                    | Instruction::ExileTargetUntilOpponentMonarch => {
                        (ObjectDestination::Exile, ZoneReason::ExiledByEffect)
                    }
                    _ => (ObjectDestination::Hand, ZoneReason::ReturnToOwnersHand),
                };
                if instruction == Instruction::ExileTarget {
                    let TargetRef::Object(object) = target else {
                        return Err(EffectError::InconsistentProgram);
                    };
                    let view = before.view();
                    let lki = view
                        .evaluated
                        .get(object)
                        .ok_or(EffectError::InconsistentProgram)?;
                    let departed = DepartedTarget {
                        controller: lki.controller.ok_or(EffectError::InconsistentProgram)?,
                        power: lki
                            .power_toughness
                            .as_ref()
                            .map(|pt| pt.derived.power.clone()),
                    };
                    self.resolution_cursor_mut()
                        .ok_or(EffectError::WrongResolutionFrame)?
                        .exiled_target = Some(departed);
                }
                let (window, exit) = self.window.record_targeted_resolution_move_with_exit(
                    self.window.revision(),
                    choice.id,
                    destination,
                )?;
                self.window = window;
                if instruction == Instruction::DestroyTarget
                    && let crate::stack_window::StackExit::Object(movement) = &exit
                    && matches!(movement.after.zone(), ObjectZone::Graveyard(_))
                {
                    self.resolution_cursor_mut()
                        .ok_or(EffectError::WrongResolutionFrame)?
                        .results
                        .destroyed_target_controller = movement.before.controller();
                }
                if let (Some(source), crate::stack_window::StackExit::Object(movement)) =
                    (linked_source, &exit)
                {
                    self.linked_exiles
                        .entry(source)
                        .or_default()
                        .extend(movement.arrivals());
                }
                if matches!(
                    instruction,
                    Instruction::ImprintTargetLand | Instruction::ImprintTargetCreature
                ) && let crate::stack_window::StackExit::Object(movement) = &exit
                {
                    self.remember_imprinted_card(entry.id(), movement);
                }
                if instruction == Instruction::ExileTargetAsAbilityDonor
                    && let crate::stack_window::StackExit::Object(movement) = &exit
                {
                    self.remember_exiled_ability_donor(entry.id(), movement);
                }
                if instruction == Instruction::ExileUntilNextEndStep
                    && let crate::stack_window::StackExit::Object(movement) = &exit
                    && movement.after.zone() == ObjectZone::Exile
                {
                    self.schedule_return_at_next_end_step(
                        entry.id(),
                        movement.arrivals().collect(),
                    )?;
                }
                if instruction == Instruction::ExileTargetUntilOpponentMonarch
                    && let crate::stack_window::StackExit::Object(movement) = &exit
                    && movement.after.zone() == ObjectZone::Exile
                {
                    self.monarch_linked_exiles
                        .extend(movement.arrivals().map(|id| (id, entry.controller())));
                }
                InstructionCut::Exit { exit, reason }
            }
        };
        self.emit_from(
            cause,
            &before,
            None,
            append,
            |previous, current, controllers| {
                let mut entry_counters = None;
                let mut entry_effect = None;
                let occurrence = match receipt {
                    InstructionCut::Ready(value) => value,
                    InstructionCut::Exit { exit, reason } => {
                        exit_occurrence_at(previous, current, exit, reason, controllers)?
                    }
                    InstructionCut::GraveyardEntry {
                        exit,
                        previous: source,
                        object,
                        counters,
                        effect,
                    } => {
                        entry_effect = effect;
                        if counters.valid_placement() {
                            entry_counters = Some((source, object, counters));
                        }
                        exit_occurrence_at(
                            previous,
                            current,
                            exit,
                            ZoneReason::ReturnFromGraveyardToBattlefield,
                            controllers,
                        )?
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
                };
                let mut rows = vec![occurrence];
                if let Some(effect) = entry_effect {
                    rows.push(RulesOccurrence::ContinuousEffectCreated {
                        effect,
                        appearances: vec![],
                    });
                }
                if let Some((source, object, added)) = entry_counters {
                    rows.push(RulesOccurrence::CountersPlaced {
                        object,
                        before: previous.characteristics.counters(object),
                        after: current.characteristics.counters(object),
                        added,
                        before_appearance: Box::new(
                            previous
                                .appearance(source)
                                .ok_or(EffectError::InconsistentProgram)?,
                        ),
                        after_appearance: Box::new(
                            current
                                .appearance(object)
                                .ok_or(EffectError::InconsistentProgram)?,
                        ),
                    });
                }
                Ok(rows)
            },
        )?;
        Ok(())
    }
}
