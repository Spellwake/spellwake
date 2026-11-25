//! One private library action owns the existing resolution cursor until commit.
use super::zone_replacement::AuthorizedZoneBatch;
use super::*;
use crate::{
    occurrence::{EventCause, RulesOccurrence, ZoneReason},
    program::{PublicZoneDestination, ZoneActionKind},
};

#[cfg(test)]
mod tests;

/// Completed library action, distinct from physical zone movements.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LibraryActionKind {
    Scry,
    Surveil,
    Mill,
}
/// Actual one-to-one public-zone receipt; native IDs are not public transport handles.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LibraryPublicMove {
    /// Exact original library incarnation.
    pub before: ObjectId,
    /// Actual new incarnation from the atomic movement receipt.
    pub after: ObjectId,
    /// Actual public destination, never an intermediate candidate destination.
    pub destination: PublicZoneDestination,
}
/// Privileged action evidence. Selection/order vectors name before incarnations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LibraryAction {
    /// Completed operation, not an inferred draw or discard.
    pub kind: LibraryActionKind,
    /// Actual recipient, independent of the resolving controller.
    pub player: SeatId,
    /// Original evaluated amount, before clamping to the actual library.
    pub requested: RulesInteger,
    /// Frozen original top segment; empty for a positive action on an empty library.
    pub looked_top_first: Vec<ObjectId>,
    /// Surviving looked cards in their chosen top-to-bottom order.
    pub top_first: Vec<ObjectId>,
    /// Scry bottom segment in final top-to-bottom order.
    pub bottom_top_first: Vec<ObjectId>,
    /// Original selected departures in frozen library top order, before replacements.
    pub selected_before_top_first: Vec<ObjectId>,
    /// Receipt-derived public movements in that same original selected order.
    pub public_zone_moves: Vec<LibraryPublicMove>,
    /// Actual owner-chosen Graveyard arrivals only, naming before incarnations.
    pub graveyard_before_top_first: Vec<ObjectId>,
}
/// Only operations that require a player partition have a library-choice frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LibraryChoiceKind {
    Search,
    Scry,
    Surveil,
}
/// Private exact partition/order request, never a production seat-safe view.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LibraryTopFrame {
    look_top: bool,
    look_reveals: bool,
    reveal_only: bool,
    explore_nonland: bool,
    choose_land_kind: bool,
    search_exile_owner: Option<SeatId>,
    search_split: Option<SearchSplitMode>,
    search_hand_graveyard: bool,
    search_battlefield_batch: bool,
    search_batch_limit: Option<usize>,
    search_exile_batch: bool,
    search_shared_land_type: bool,
    search_forest_and_plains: bool,
    search_required: bool,
    search_entry_colors: BTreeMap<ObjectId, Vec<crate::mana::ManaType>>,
    search_entry_reveals: BTreeMap<ObjectId, Vec<ObjectId>>,
    revision: u64,
    entry: StackEntryId,
    instruction: usize,
    chooser: SeatId,
    kind: LibraryChoiceKind,
    domain_top_first: Vec<ObjectId>,
    eligible: Vec<ObjectId>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum SearchSplitMode {
    TwoWithBattlefieldFirst,
    OneWithChoiceOfDestination,
}

pub(super) struct SearchFrameMode {
    pub(super) exile_owner: Option<SeatId>,
    pub(super) split: Option<SearchSplitMode>,
    pub(super) hand_graveyard: bool,
    pub(super) battlefield_batch: bool,
    pub(super) hand_limit: Option<usize>,
    pub(super) exile_batch: bool,
    pub(super) shared_land_type: bool,
    pub(super) forest_and_plains: bool,
    pub(super) required: bool,
    pub(super) entry_colors: BTreeMap<ObjectId, Vec<crate::mana::ManaType>>,
    pub(super) entry_reveals: BTreeMap<ObjectId, Vec<ObjectId>>,
}
impl LibraryTopFrame {
    pub(super) fn search(
        revision: u64,
        entry: StackEntryId,
        instruction: usize,
        chooser: SeatId,
        domain_top_first: Vec<ObjectId>,
        eligible: Vec<ObjectId>,
        mode: SearchFrameMode,
    ) -> Self {
        Self {
            look_top: false,
            look_reveals: false,
            reveal_only: false,
            explore_nonland: false,
            choose_land_kind: false,
            revision,
            entry,
            instruction,
            chooser,
            kind: LibraryChoiceKind::Search,
            search_exile_owner: mode.exile_owner,
            search_split: mode.split,
            search_hand_graveyard: mode.hand_graveyard,
            search_battlefield_batch: mode.battlefield_batch,
            search_batch_limit: mode.hand_limit,
            search_exile_batch: mode.exile_batch,
            search_shared_land_type: mode.shared_land_type,
            search_forest_and_plains: mode.forest_and_plains,
            search_required: mode.required,
            search_entry_colors: mode.entry_colors,
            search_entry_reveals: mode.entry_reveals,
            domain_top_first,
            eligible,
        }
    }
    // Internal frame constructor forwarding top-library look configuration fields.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn looked_top(
        revision: u64,
        entry: StackEntryId,
        instruction: usize,
        chooser: SeatId,
        domain: Vec<ObjectId>,
        eligible: Vec<ObjectId>,
        required: bool,
        reveal: bool,
        battlefield_pair: bool,
        choose_land_kind: bool,
        hand_limit: Option<usize>,
        explore_nonland: bool,
    ) -> Self {
        let mut frame = Self::search(
            revision,
            entry,
            instruction,
            chooser,
            domain,
            eligible,
            SearchFrameMode {
                exile_owner: None,
                split: None,
                hand_graveyard: false,
                battlefield_batch: battlefield_pair,
                hand_limit,
                exile_batch: false,
                shared_land_type: false,
                forest_and_plains: false,
                required,
                entry_colors: BTreeMap::new(),
                entry_reveals: BTreeMap::new(),
            },
        );
        frame.choose_land_kind = choose_land_kind;
        frame.look_top = true;
        frame.explore_nonland = explore_nonland;
        frame.look_reveals = reveal;
        frame
    }
    pub(super) fn with_reveal_only(mut self, reveal_only: bool) -> Self {
        self.reveal_only = reveal_only;
        self
    }

    /// The selected card is revealed and remains in its current position.
    pub fn is_reveal_only(&self) -> bool {
        self.reveal_only
    }

    /// This frame inspects only a fixed top segment and does not search the library.
    pub fn is_look_top(&self) -> bool {
        self.look_top
    }
    /// The controller chooses a card kind before seeing any library cards.
    pub fn chooses_land_kind(&self) -> bool {
        self.choose_land_kind
    }
    /// Whether the selected card is revealed to all players before it moves.
    pub fn looked_card_is_revealed(&self) -> bool {
        self.look_reveals
    }
    /// The library whose controller shuffles after this unrestricted search.
    pub fn search_exile_owner(&self) -> Option<SeatId> {
        self.search_exile_owner
    }
    pub fn requires_search_result(&self) -> bool {
        self.search_required
    }
    /// Whether the search expects separately assigned battlefield and hand results.
    pub fn is_split_search(&self) -> bool {
        self.search_split.is_some()
    }
    /// Whether the chooser may send a single result to either battlefield or hand.
    pub fn offers_search_destination(&self) -> bool {
        self.search_split == Some(SearchSplitMode::OneWithChoiceOfDestination)
    }
    /// Search assigns distinct cards to hand and graveyard.
    pub fn is_hand_graveyard_search(&self) -> bool {
        self.search_hand_graveyard
    }
    /// Whether this search chooses up to two battlefield results in timestamp order.
    pub fn is_battlefield_batch_search(&self) -> bool {
        self.search_battlefield_batch
    }
    /// Whether the controller is choosing the destination of a publicly revealed explored card.
    pub fn may_put_explored_card_in_graveyard(&self) -> bool {
        self.explore_nonland
    }
    /// Whether the selected cards move to linked exile.
    pub fn is_exile_batch_search(&self) -> bool {
        self.search_exile_batch
    }
    /// Whether a single private reply selects a bounded number of cards.
    pub fn is_bounded_batch_search(&self) -> bool {
        self.search_batch_limit.is_some()
    }
    /// Maximum number of cards the controller may select in this batch.
    pub fn search_batch_limit(&self) -> usize {
        self.search_batch_limit.unwrap_or(0)
    }
    /// Whether two selected cards must share at least one land subtype.
    pub fn requires_shared_land_type(&self) -> bool {
        self.search_shared_land_type
    }
    /// Whether two selected cards must be assignable to distinct Forest and Plains slots.
    pub fn requires_forest_and_plains(&self) -> bool {
        self.search_forest_and_plains
    }
    /// Color choices keyed by eligible original library incarnation, when that card enters.
    pub fn search_entry_color_options(&self) -> &BTreeMap<ObjectId, Vec<crate::mana::ManaType>> {
        &self.search_entry_colors
    }
    /// Private eligible reveal cards keyed by the original entering library object.
    pub fn search_entry_reveal_options(&self) -> &BTreeMap<ObjectId, Vec<ObjectId>> {
        &self.search_entry_reveals
    }
    /// Executor revision that authenticates this frame.
    pub fn revision(&self) -> u64 {
        self.revision
    }
    /// Actual resolving entry owning the action.
    pub fn entry(&self) -> StackEntryId {
        self.entry
    }
    /// Absolute instruction index in the retained body.
    pub fn instruction(&self) -> usize {
        self.instruction
    }
    /// Recipient who owns this private library choice.
    pub fn chooser(&self) -> SeatId {
        self.chooser
    }
    /// Required reply family.
    pub fn kind(&self) -> LibraryChoiceKind {
        self.kind
    }
    /// Eligible object incarnations for this private library search.
    pub fn eligible_results(&self) -> &[ObjectId] {
        &self.eligible
    }
    /// Privately inspected cards in library order.
    pub fn domain_top_first(&self) -> &[ObjectId] {
        &self.domain_top_first
    }
}
/// Both Scry vectors are top-to-bottom; Surveil's graveyard vector selects only.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LibraryTopReply {
    /// One eligible card or an intentional failure to find.
    Search { selected: Option<ObjectId> },
    /// Up to two distinct results; a lone result must be the battlefield choice.
    SearchSplit {
        battlefield: Option<ObjectId>,
        hand: Option<ObjectId>,
    },
    /// Exact independent hand and graveyard assignments.
    SearchHandGraveyard {
        hand: Option<ObjectId>,
        graveyard: Option<ObjectId>,
    },
    /// Distinct results in the controller's chosen relative timestamp order.
    SearchBattlefield {
        selected_in_timestamp_order: Vec<ObjectId>,
    },
    /// A search selection with explicit pre-entry colors keyed by selected library objects.
    /// For a split search, the first selection enters and the optional second goes to hand;
    /// for battlefield batches, selections are in relative timestamp order.
    SearchWithEntryColors {
        selected: Vec<ObjectId>,
        colors: Vec<(ObjectId, crate::mana::ManaType)>,
    },
    /// Search results with every required entry decision keyed by its original object.
    SearchWithEntryChoices {
        selected: Vec<ObjectId>,
        choices: Vec<(ObjectId, crate::land::EntryChoice)>,
    },
    Scry {
        top_first: Vec<ObjectId>,
        bottom_top_first: Vec<ObjectId>,
    },
    Surveil {
        top_first: Vec<ObjectId>,
        to_graveyard: Vec<ObjectId>,
    },
}
#[derive(Debug, Clone, PartialEq, Eq)]
enum LibraryStage {
    Choosing,
    Preparing {
        top_first: Vec<ObjectId>,
        bottom_top_first: Vec<ObjectId>,
        selected: Vec<ObjectId>,
    },
    Replacing {
        top_first: Vec<ObjectId>,
        bottom_top_first: Vec<ObjectId>,
        selected: Vec<ObjectId>,
        batch: Box<ZoneReplacementBatch>,
    },
    Ordering {
        top_first: Vec<ObjectId>,
        bottom_top_first: Vec<ObjectId>,
        selected: Vec<ObjectId>,
        replacements: Option<Box<ZoneReplacementBatch>>,
        groups: Vec<ResolutionGraveyardGroup>,
        orders: Vec<ResolutionAcceptedGraveyardOrder>,
    },
    ApplyEventReplacement(Box<super::event_replacement::EventBodyReplacement>),
    OptionalEventReplacement {
        replacement: Box<super::event_replacement::EventBodyReplacement>,
        declined: BTreeSet<(Option<ObjectId>, crate::characteristics::StaticAbilityId)>,
    },
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct LibraryTopWork {
    pub(super) cursor: ResolutionCursor,
    kind: LibraryActionKind,
    player: SeatId,
    requested: RulesInteger,
    domain: Vec<ObjectId>,
    library_before: Vec<ObjectId>,
    window_revision: u64,
    stage: LibraryStage,
}
impl LibraryTopWork {
    fn frame(&self, revision: u64) -> Option<EffectDecision> {
        match &self.stage {
            LibraryStage::Choosing => {
                let kind = match self.kind {
                    LibraryActionKind::Scry => LibraryChoiceKind::Scry,
                    LibraryActionKind::Surveil => LibraryChoiceKind::Surveil,
                    LibraryActionKind::Mill => return None,
                };
                Some(EffectDecision::LibraryTop(LibraryTopFrame {
                    look_top: false,
                    look_reveals: false,
                    reveal_only: false,
                    explore_nonland: false,
                    choose_land_kind: false,
                    search_exile_owner: None,
                    search_split: None,
                    search_hand_graveyard: false,
                    search_battlefield_batch: false,
                    search_batch_limit: None,
                    search_exile_batch: false,
                    search_shared_land_type: false,
                    search_forest_and_plains: false,
                    search_required: false,
                    search_entry_colors: BTreeMap::new(),
                    search_entry_reveals: BTreeMap::new(),
                    revision,
                    entry: self.cursor.entry,
                    instruction: self.cursor.instruction,
                    chooser: self.player,
                    kind,
                    domain_top_first: self.domain.clone(),
                    eligible: Vec::new(),
                }))
            }
            LibraryStage::Preparing { .. } => None,
            LibraryStage::Replacing { batch, .. } => {
                batch.frame(revision).map(EffectDecision::ZoneReplacement)
            }
            LibraryStage::Ordering { groups, orders, .. } => {
                groups.get(orders.len()).map(|group| {
                    EffectDecision::ResolutionGraveyardOrder(
                        ResolutionGraveyardOrderFrame::for_group(
                            revision,
                            self.cursor.entry,
                            self.cursor.instruction,
                            group,
                        ),
                    )
                })
            }
            LibraryStage::ApplyEventReplacement(_) => None,
            LibraryStage::OptionalEventReplacement { replacement, .. } => {
                Some(EffectDecision::OptionalEffect(
                    OptionalEffectFrame::event_replacement(revision, replacement.affected_player),
                ))
            }
        }
    }
}
impl EffectWindow {
    /// Reissue an uncommitted scry or surveil partition at the new object cut.
    pub(super) fn library_departure_admitted(
        &self,
        issued: Option<&EffectDecision>,
    ) -> Result<bool, EffectError> {
        let Some(PendingWork::LibraryTop(work)) = &self.pending_work else {
            return Ok(false);
        };
        if !matches!(work.stage, LibraryStage::Choosing)
            || issued.is_none()
            || self.library_top_frame().as_ref() != issued
        {
            return Ok(false);
        }
        self.validate_library_authority(work)?;
        Ok(true)
    }

    pub(super) fn refresh_library_top_after_departure(&mut self) -> Result<(), EffectError> {
        let Some(PendingWork::LibraryTop(mut work)) = self.pending_work.take() else {
            return Err(EffectError::InconsistentLibraryTopWork);
        };
        if !self.window.participants().contains(&work.player) {
            // This exact library no longer exists. Continue the saved spell cursor.
            self.pending_work = Some(PendingWork::Resolution(work.cursor));
            return self.advance_instruction();
        }
        work.window_revision = self.window.revision();
        self.validate_library_authority(&work)?;
        self.pending_work = Some(PendingWork::LibraryTop(work));
        Ok(())
    }

    pub(super) fn library_top_frame(&self) -> Option<EffectDecision> {
        match self.pending_work.as_ref() {
            Some(PendingWork::LibraryTop(work)) => work.frame(self.revision),
            _ => None,
        }
    }
    pub(super) fn begin_library_top(
        &mut self,
        instruction: &Instruction,
    ) -> Result<bool, EffectError> {
        let (kind, role, amount) = match instruction {
            Instruction::Scry { player, amount } => (LibraryActionKind::Scry, *player, amount),
            Instruction::Surveil { player, amount } => {
                (LibraryActionKind::Surveil, *player, amount)
            }
            Instruction::Mill { player, amount } => (LibraryActionKind::Mill, *player, amount),
            Instruction::BranchIfFalse { .. }
            | Instruction::Jump { .. }
            | Instruction::OptionalSequence { .. }
            | Instruction::OptionalEffect { .. }
            | Instruction::OfferManaPayment { .. }
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
            | Instruction::OfferDynamicManaPayment { .. }
            | Instruction::PayLife { .. }
            | Instruction::PayEnergy { .. }
            | Instruction::ExileFromZone { .. }
            | Instruction::TapOrUntapControlled { .. }
            | Instruction::ExertAbilitySource
            | Instruction::CollectEvidence { .. }
            | Instruction::Blight { .. }
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
            | Instruction::CounterTriggeringStackObject
            | Instruction::BoostSourceByCastManaValue
            | Instruction::BoostLoneAttacker
            | Instruction::Attach(_)
            | Instruction::GoadTarget
            | Instruction::RequireTargetAttack
            | Instruction::RemoveTargetFromCombat
            | Instruction::DestroyTarget
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
            | Instruction::DrawTargetPlayer { .. }
            | Instruction::DrawHighestEntrantController
            | Instruction::DrawRelatedPlayer
            | Instruction::DrawCombatDamageSourceController
            | Instruction::DrawAndLoseLifeForTargetGraveyardCreatures
            | Instruction::DrawControllerAndTarget { .. }
            | Instruction::DrawEachPlayer { .. }
            | Instruction::DrawPlayersControllingName(_)
            | Instruction::DrawController { .. }
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
            | Instruction::DestroyTargetWithoutRegeneration
            | Instruction::DrawPlayer { .. }
            | Instruction::DiscardFiltered { .. } => return Ok(false),
        };
        let controller = self
            .window
            .resolving()
            .ok_or(EffectError::WrongResolutionFrame)?
            .controller();
        let raw_requested = self.resolve_value_expr(controller, amount)?;
        if raw_requested < 0.into() {
            return Err(EffectError::InconsistentLibraryTopWork);
        }
        let player = self.resolving_player(role)?;
        let requested = self.adjusted_library_action_amount(kind, player, raw_requested.clone())?;
        let library_before = self.window.cards().library(player).to_vec();
        let library = &library_before;
        let count = std::cmp::min(&requested, &RulesInteger::from_usize(library.len()))
            .to_usize()
            .ok_or(EffectError::InconsistentLibraryTopWork)?;
        let domain = library[..count].to_vec();
        let event_replacement = if raw_requested.is_positive() {
            let event_kind = match kind {
                LibraryActionKind::Scry => {
                    Some(super::event_replacement::ReplacementEventKind::Scry)
                }
                LibraryActionKind::Mill => {
                    Some(super::event_replacement::ReplacementEventKind::Mill)
                }
                LibraryActionKind::Surveil => None,
            };
            if let Some(ev) = event_kind {
                let evaluated = self.evaluated_characteristics()?;
                self.find_event_body_replacement(&evaluated, ev, player, &BTreeSet::new())?
            } else {
                None
            }
        } else {
            None
        };
        let stage = if let Some(rep) = event_replacement {
            if rep.optional {
                LibraryStage::OptionalEventReplacement {
                    replacement: Box::new(rep),
                    declined: BTreeSet::new(),
                }
            } else {
                LibraryStage::ApplyEventReplacement(Box::new(rep))
            }
        } else if kind != LibraryActionKind::Mill && !domain.is_empty() {
            LibraryStage::Choosing
        } else {
            let selected = if kind == LibraryActionKind::Mill {
                domain.clone()
            } else {
                vec![]
            };
            LibraryStage::Preparing {
                top_first: vec![],
                bottom_top_first: vec![],
                selected,
            }
        };
        let Some(PendingWork::Resolution(cursor)) = self.pending_work.take() else {
            return Err(EffectError::WrongResolutionFrame);
        };
        self.pending_work = Some(PendingWork::LibraryTop(Box::new(LibraryTopWork {
            cursor,
            kind,
            player,
            requested,
            domain,
            library_before,
            window_revision: self.window.revision(),
            stage,
        })));
        Ok(true)
    }
    pub(super) fn progress_library_top(
        &mut self,
        append: &mut RulesJournal,
    ) -> Result<Option<EffectDecision>, EffectError> {
        let Some(PendingWork::LibraryTop(work)) = self.pending_work.take() else {
            return Err(EffectError::WrongLibraryTopFrame);
        };
        let mut work = *work;
        loop {
            // The temporary stage is not published; the single owned work/cursor
            // is reinstalled only with the actual next stage or completed result.
            match std::mem::replace(&mut work.stage, LibraryStage::Choosing) {
                LibraryStage::Choosing => {
                    work.stage = LibraryStage::Choosing;
                }
                LibraryStage::Preparing {
                    top_first,
                    bottom_top_first,
                    selected,
                } => {
                    self.validate_library_partition(
                        &work,
                        &top_first,
                        &bottom_top_first,
                        &selected,
                    )?;
                    let replacements = self.begin_library_replacements(&work, &selected)?;
                    if let Some(batch) = replacements {
                        work.stage = LibraryStage::Replacing {
                            top_first,
                            bottom_top_first,
                            selected,
                            batch,
                        };
                    } else {
                        let groups = self.selection_arrival_groups(
                            &[ResolutionAcceptedSelection {
                                player: work.player,
                                objects: selected.clone(),
                            }],
                            None,
                        )?;
                        work.stage = LibraryStage::Ordering {
                            top_first,
                            bottom_top_first,
                            selected,
                            replacements: None,
                            groups,
                            orders: vec![],
                        };
                    }
                    continue;
                }
                LibraryStage::Replacing {
                    top_first,
                    bottom_top_first,
                    selected,
                    mut batch,
                } => {
                    let frame = batch.progress(self.revision)?;
                    if frame.is_some() {
                        work.stage = LibraryStage::Replacing {
                            top_first,
                            bottom_top_first,
                            selected,
                            batch,
                        };
                        self.pending_work = Some(PendingWork::LibraryTop(Box::new(work)));
                        return Ok(frame);
                    }
                    let groups = self.selection_arrival_groups(
                        &[ResolutionAcceptedSelection {
                            player: work.player,
                            objects: selected.clone(),
                        }],
                        Some(&batch),
                    )?;
                    work.stage = LibraryStage::Ordering {
                        top_first,
                        bottom_top_first,
                        selected,
                        replacements: Some(batch),
                        groups,
                        orders: vec![],
                    };
                    continue;
                }
                LibraryStage::Ordering {
                    top_first,
                    bottom_top_first,
                    selected,
                    replacements,
                    groups,
                    mut orders,
                } => {
                    let complete =
                        resolution_selection::next_graveyard_group(&groups, &mut orders)?.is_none();
                    work.stage = LibraryStage::Ordering {
                        top_first,
                        bottom_top_first,
                        selected,
                        replacements,
                        groups,
                        orders,
                    };
                    if complete {
                        self.commit_library_top(&mut work, append)?;
                        self.pending_work = Some(PendingWork::Resolution(work.cursor));
                        self.advance_instruction()?;
                        return Ok(None);
                    }
                }
                LibraryStage::ApplyEventReplacement(rep) => {
                    let cause = EventCause::Instruction {
                        entry: work.cursor.entry,
                        instruction: work.cursor.instruction,
                    };
                    self.pending_work = Some(PendingWork::Resolution(work.cursor));
                    self.execute_inline_replacement_body(&rep, cause, append)?;
                    self.advance_instruction()?;
                    return Ok(None);
                }
                LibraryStage::OptionalEventReplacement {
                    replacement,
                    declined,
                } => {
                    work.stage = LibraryStage::OptionalEventReplacement {
                        replacement,
                        declined,
                    };
                }
            }
            let frame = work
                .frame(self.revision)
                .ok_or(EffectError::InconsistentLibraryTopWork)?;
            self.pending_work = Some(PendingWork::LibraryTop(Box::new(work)));
            return Ok(Some(frame));
        }
    }
    /// Resume only the genuine current native library/owner frame on a tentative clone.
    ///
    /// # Errors
    /// Rejects a stale frame/revision, wrong actor/kind, incomplete or duplicated
    /// partition, or invalid owner permutation. Later commit failures are atomic.
    pub(super) fn resume_library_top(
        &self,
        expected: u64,
        frame: &EffectDecision,
        actor: SeatId,
        reply: DecisionReply,
    ) -> Result<RuleProgress, EffectError> {
        let revision = self.next_revision(expected)?;
        let Some(PendingWork::LibraryTop(owned)) = &self.pending_work else {
            return Err(EffectError::WrongLibraryTopFrame);
        };
        self.validate_library_authority(owned)?;
        if self.library_top_frame().as_ref() != Some(frame) {
            return Err(EffectError::WrongLibraryTopFrame);
        }
        if actor != frame.chooser() {
            return Err(EffectError::WrongChooser);
        }
        let mut next = self.clone();
        let Some(PendingWork::LibraryTop(work)) = next.pending_work.take() else {
            return Err(EffectError::WrongLibraryTopFrame);
        };
        let mut work = *work;
        work.stage = match (work.stage, reply) {
            (LibraryStage::Choosing, DecisionReply::LibraryTop(reply)) => {
                let (top_first, bottom_top_first, selected) = match (work.kind, reply) {
                    (
                        LibraryActionKind::Scry,
                        LibraryTopReply::Scry {
                            top_first,
                            bottom_top_first,
                        },
                    ) => {
                        validate_partition(&work.domain, &top_first, &bottom_top_first)?;
                        (top_first, bottom_top_first, vec![])
                    }
                    (
                        LibraryActionKind::Surveil,
                        LibraryTopReply::Surveil {
                            top_first,
                            to_graveyard,
                        },
                    ) => {
                        validate_partition(&work.domain, &top_first, &to_graveyard)?;
                        (top_first, vec![], to_graveyard)
                    }
                    _ => return Err(EffectError::WrongDecisionKind),
                };
                let selected = work
                    .domain
                    .iter()
                    .copied()
                    .filter(|object| selected.contains(object))
                    .collect();
                LibraryStage::Preparing {
                    top_first,
                    bottom_top_first,
                    selected,
                }
            }
            (
                LibraryStage::Ordering {
                    top_first,
                    bottom_top_first,
                    selected,
                    replacements,
                    groups,
                    mut orders,
                },
                DecisionReply::OrderResolutionGraveyard(order),
            ) => {
                let group = groups
                    .get(orders.len())
                    .ok_or(EffectError::InconsistentLibraryTopWork)?;
                resolution_selection::validate_graveyard_order(group, &order)?;
                orders.push(ResolutionAcceptedGraveyardOrder {
                    owner: actor,
                    top_to_bottom: order,
                });
                LibraryStage::Ordering {
                    top_first,
                    bottom_top_first,
                    selected,
                    replacements,
                    groups,
                    orders,
                }
            }
            (
                LibraryStage::Replacing {
                    top_first,
                    bottom_top_first,
                    selected,
                    mut batch,
                },
                DecisionReply::ZoneReplacement(reply),
            ) => {
                let EffectDecision::ZoneReplacement(bound) = frame else {
                    return Err(EffectError::WrongLibraryTopFrame);
                };
                batch.accept(self.revision, bound, actor, reply)?;
                LibraryStage::Replacing {
                    top_first,
                    bottom_top_first,
                    selected,
                    batch,
                }
            }
            (
                LibraryStage::OptionalEventReplacement {
                    replacement,
                    mut declined,
                },
                DecisionReply::OptionalEffect(choice),
            ) => match choice {
                OptionalEffectChoice::Take => LibraryStage::ApplyEventReplacement(replacement),
                OptionalEffectChoice::Decline => {
                    declined.insert((replacement.source, replacement.ability));
                    let event_kind = match work.kind {
                        LibraryActionKind::Scry => {
                            Some(super::event_replacement::ReplacementEventKind::Scry)
                        }
                        LibraryActionKind::Mill => {
                            Some(super::event_replacement::ReplacementEventKind::Mill)
                        }
                        LibraryActionKind::Surveil => None,
                    };
                    let next_rep = if let Some(ev) = event_kind {
                        let evaluated = next.evaluated_characteristics()?;
                        next.find_event_body_replacement(&evaluated, ev, work.player, &declined)?
                    } else {
                        None
                    };
                    if let Some(rep) = next_rep {
                        if rep.optional {
                            LibraryStage::OptionalEventReplacement {
                                replacement: Box::new(rep),
                                declined,
                            }
                        } else {
                            LibraryStage::ApplyEventReplacement(Box::new(rep))
                        }
                    } else if work.kind != LibraryActionKind::Mill && !work.domain.is_empty() {
                        LibraryStage::Choosing
                    } else {
                        let selected = if work.kind == LibraryActionKind::Mill {
                            work.domain.clone()
                        } else {
                            vec![]
                        };
                        LibraryStage::Preparing {
                            top_first: vec![],
                            bottom_top_first: vec![],
                            selected,
                        }
                    }
                }
                _ => return Err(EffectError::IllegalOptionalEffect),
            },
            _ => return Err(EffectError::WrongDecisionKind),
        };
        next.pending_work = Some(PendingWork::LibraryTop(Box::new(work)));
        next.revision = revision;
        next.progress_rules(
            DamageJournal::default(),
            vec![],
            vec![],
            DrawJournal::default(),
            RulesJournal::default(),
        )
    }
    pub(super) fn adjusted_library_action_amount(
        &self,
        kind: LibraryActionKind,
        player: SeatId,
        mut amount: RulesInteger,
    ) -> Result<RulesInteger, EffectError> {
        if !amount.is_positive()
            || !matches!(kind, LibraryActionKind::Scry | LibraryActionKind::Mill)
        {
            return Ok(amount);
        }
        let evaluated = self.evaluated_characteristics()?;
        let mut total_add = 0i64;
        let mut factors: Vec<u8> = Vec::new();
        let mut skip = false;
        let check_rule = |ctrl: SeatId,
                          ab: &crate::characteristics::StaticContinuousAbility,
                          total_add: &mut i64,
                          factors: &mut Vec<u8>,
                          skip: &mut bool| {
            let crate::characteristics::StaticContinuousAbility::EventReplacement {
                event,
                action,
                ..
            } = ab
            else {
                return;
            };
            let rel_opt = match (kind, event) {
                (
                    LibraryActionKind::Scry,
                    crate::characteristics::ReplaceableEvent::Scry { player: rel },
                )
                | (
                    LibraryActionKind::Mill,
                    crate::characteristics::ReplaceableEvent::Mill { player: rel },
                ) => Some(rel),
                _ => None,
            };
            if let Some(rel) = rel_opt {
                let applies = match rel {
                    crate::program::AffectedPlayers::One(
                        crate::program::ResolvingPlayer::Controller,
                    ) => player == ctrl,
                    crate::program::AffectedPlayers::EachOpponent
                    | crate::program::AffectedPlayers::EachOtherPlayer => player != ctrl,
                    crate::program::AffectedPlayers::EachPlayer
                    | crate::program::AffectedPlayers::One(_) => true,
                };
                if applies {
                    match action {
                        crate::characteristics::StaticReplacementAction::ModifyAmount {
                            add,
                            multiply,
                        } => {
                            if let crate::program::ValueExpr::Literal(v) = add.as_ref()
                                && let Some(u) = v.to_usize()
                            {
                                *total_add =
                                    total_add.saturating_add(i64::try_from(u).unwrap_or(i64::MAX));
                            }
                            if *multiply > 1 {
                                factors.push(*multiply);
                            }
                        }
                        crate::characteristics::StaticReplacementAction::Skip => {
                            *skip = true;
                        }
                        crate::characteristics::StaticReplacementAction::ReplaceWithBody {
                            ..
                        } => {
                            // Whole-event body replacements execute via LibraryStage::{ApplyEventReplacement,
                            // OptionalEventReplacement} in begin_library_top/progress_library_top rather than
                            // scaling the numeric count here.
                        }
                    }
                }
            }
        };
        for perm in evaluated
            .objects
            .values()
            .filter(|p| p.zone == ObjectZone::Battlefield)
        {
            let Some(ctrl) = perm.controller else {
                continue;
            };
            for ab in perm
                .printed_text
                .continuous
                .iter()
                .chain(&perm.printed_text.additional_battlefield_abilities)
            {
                let active = perm.abilities.continuous == Some(ab.id())
                    || perm
                        .abilities
                        .additional_battlefield_abilities
                        .contains(&ab.id());
                if active {
                    check_rule(ctrl, ab, &mut total_add, &mut factors, &mut skip);
                }
            }
        }
        for active in &self.temporary_rules {
            if !active.consumed
                && let crate::program::TemporaryRuleKind::TemporaryStatic(ab) = &active.rule
            {
                check_rule(
                    active.controller,
                    ab,
                    &mut total_add,
                    &mut factors,
                    &mut skip,
                );
            }
        }
        if skip {
            return Ok(0.into());
        }
        if total_add != 0 {
            amount = amount.plus(&RulesInteger::from(total_add)).max(0.into());
        }
        for factor in factors {
            amount = amount.times(&RulesInteger::from(i64::from(factor)));
        }
        Ok(amount)
    }
    /// Reauthorize the exact original operation/amount/prefix in every branch.
    fn validate_library_authority(&self, work: &LibraryTopWork) -> Result<(), EffectError> {
        let entry = self
            .window
            .resolving()
            .filter(|entry| entry.id() == work.cursor.entry)
            .ok_or(EffectError::WrongResolutionFrame)?;
        let actual = self
            .payloads
            .get(&work.cursor.entry)
            .and_then(StackPayload::instructions)
            .and_then(|body| body.get(work.cursor.instruction))
            .ok_or(EffectError::InconsistentLibraryTopWork)?;
        let (kind, role, amount) = match actual {
            Instruction::Scry { player, amount } => (LibraryActionKind::Scry, *player, amount),
            Instruction::Surveil { player, amount } => {
                (LibraryActionKind::Surveil, *player, amount)
            }
            Instruction::Mill { player, amount } => (LibraryActionKind::Mill, *player, amount),
            _ => return Err(EffectError::InconsistentLibraryTopWork),
        };
        let raw_requested =
            self.resolve_value_expr_at(entry.controller(), amount, Some(&work.cursor))?;
        let resolved_player = self.resolving_player_at(role, Some(&work.cursor))?;
        let requested = if raw_requested < 0.into() {
            raw_requested
        } else {
            self.adjusted_library_action_amount(kind, resolved_player, raw_requested)?
        };
        if kind != work.kind
            || resolved_player != work.player
            || requested < 0.into()
            || requested != work.requested
            || self.window.revision() != work.window_revision
            || self.window.cards().library(work.player) != work.library_before
        {
            return Err(EffectError::InconsistentLibraryTopWork);
        }
        let count = std::cmp::min(
            &requested,
            &RulesInteger::from_usize(work.library_before.len()),
        )
        .to_usize()
        .ok_or(EffectError::InconsistentLibraryTopWork)?;
        if work.domain != work.library_before[..count]
            || work.domain.iter().any(|id| {
                self.window.cards().get(*id).is_none_or(|c| {
                    c.zone() != ObjectZone::Library(work.player)
                        || c.owner() != work.player
                        || c.card_id().is_none()
                })
            })
        {
            return Err(EffectError::InconsistentLibraryTopWork);
        }
        Ok(())
    }
    fn validate_library_partition(
        &self,
        work: &LibraryTopWork,
        top: &[ObjectId],
        bottom: &[ObjectId],
        selected: &[ObjectId],
    ) -> Result<(), EffectError> {
        self.validate_library_authority(work)?;
        match work.kind {
            LibraryActionKind::Scry if selected.is_empty() => {
                validate_partition(&work.domain, top, bottom)
            }
            LibraryActionKind::Surveil if bottom.is_empty() => {
                validate_partition(&work.domain, top, selected)
            }
            LibraryActionKind::Mill
                if top.is_empty() && bottom.is_empty() && selected == work.domain =>
            {
                Ok(())
            }
            _ => Err(EffectError::InconsistentLibraryTopWork),
        }
    }
    fn begin_library_replacements(
        &self,
        work: &LibraryTopWork,
        selected: &[ObjectId],
    ) -> Result<Option<Box<ZoneReplacementBatch>>, EffectError> {
        self.validate_library_authority(work)?;
        if selected.is_empty() {
            return Ok(None);
        }
        if selected.iter().copied().collect::<BTreeSet<_>>().len() != selected.len()
            || !selected.iter().all(|id| work.domain.contains(id))
        {
            return Err(EffectError::InconsistentLibraryTopWork);
        }
        let (action, reason) = match work.kind {
            LibraryActionKind::Mill if selected == work.domain => (
                ZoneActionKind::EffectMill,
                ZoneReason::Mill {
                    player: work.player,
                },
            ),
            LibraryActionKind::Surveil => (
                ZoneActionKind::EffectSurveil,
                ZoneReason::Surveil {
                    player: work.player,
                },
            ),
            _ => return Err(EffectError::InconsistentLibraryTopWork),
        };
        let batch = self.begin_zone_replacements(AuthorizedZoneBatch {
            action,
            cause: EventCause::Instruction {
                entry: work.cursor.entry,
                instruction: work.cursor.instruction,
            },
            intents: selected
                .iter()
                .map(|before| ZoneMoveIntent {
                    before: *before,
                    reason,
                })
                .collect(),
        })?;
        if batch.sources().is_empty() {
            return Ok(None);
        }
        if batch.candidates().len() != selected.len() {
            return Err(EffectError::InconsistentLibraryTopWork);
        }
        Ok(Some(Box::new(batch)))
    }
    fn commit_library_top(
        &mut self,
        work: &mut LibraryTopWork,
        append: &mut RulesJournal,
    ) -> Result<(), EffectError> {
        self.validate_library_authority(work)?;
        if work.kind == LibraryActionKind::Mill {
            work.cursor.results.milled_mana_values.clear();
        }
        let LibraryStage::Ordering {
            top_first,
            bottom_top_first,
            selected,
            orders,
            replacements,
            ..
        } = &work.stage
        else {
            return Err(EffectError::InconsistentLibraryTopWork);
        };
        self.validate_library_partition(work, top_first, bottom_top_first, selected)?;
        let fresh = self.begin_library_replacements(work, selected)?;
        match (replacements, fresh) {
            (None, None) => {}
            (Some(retained), Some(initial)) => retained.verify_initial(*initial)?,
            _ => return Err(EffectError::InconsistentLibraryTopWork),
        }
        if !work.requested.is_positive() {
            return Ok(());
        }
        let batch = self.selection_batch_replaced(
            &[ResolutionAcceptedSelection {
                player: work.player,
                objects: selected.clone(),
            }],
            orders.clone(),
            replacements.clone(),
        )?;
        let before = self.rules_snapshot(None)?;
        let application = batch
            .application
            .iter()
            .map(|row| {
                Ok(crate::stack_window::LibraryTopMove {
                    before: row.object,
                    destination: batch
                        .replacements
                        .as_ref()
                        .map_or(Ok(PublicZoneDestination::Graveyard), |work| {
                            work.destination(row.object)
                        })?,
                })
            })
            .collect::<Result<Vec<_>, EffectError>>()?;
        let (window, movements) = self.window.record_library_top(
            self.window.revision(),
            work.cursor.entry,
            crate::stack_window::LibraryTopCommit {
                player: work.player,
                frozen: &work.domain,
                top: top_first,
                bottom: bottom_top_first,
                moves_application: &application,
            },
        )?;
        let count = movements.len();
        let by_id: BTreeMap<_, _> = movements
            .into_iter()
            .map(|movement| (movement.before.id(), movement))
            .collect();
        if count != selected.len()
            || by_id.len() != count
            || by_id.keys().copied().collect::<BTreeSet<_>>() != selected.iter().copied().collect()
        {
            return Err(EffectError::InconsistentLibraryTopWork);
        }
        let public_zone_moves = selected
            .iter()
            .map(|id| {
                let movement = by_id
                    .get(id)
                    .ok_or(EffectError::InconsistentLibraryTopWork)?;
                let destination = match movement.after.zone() {
                    ObjectZone::Graveyard(owner) if owner == work.player => {
                        PublicZoneDestination::Graveyard
                    }
                    ObjectZone::Exile => PublicZoneDestination::Exile,
                    _ => return Err(EffectError::InconsistentLibraryTopWork),
                };
                if batch
                    .replacements
                    .as_ref()
                    .map_or(Ok(PublicZoneDestination::Graveyard), |work| {
                        work.destination(*id)
                    })?
                    != destination
                {
                    return Err(EffectError::InconsistentLibraryTopWork);
                }
                Ok(LibraryPublicMove {
                    before: *id,
                    after: movement.after.id(),
                    destination,
                })
            })
            .collect::<Result<Vec<_>, EffectError>>()?;
        self.window = window;
        if work.kind == LibraryActionKind::Mill {
            work.cursor.results.last_action_count =
                Some(RulesInteger::from_usize(public_zone_moves.len()));
            work.cursor.results.last_action_succeeded = !public_zone_moves.is_empty();
            work.cursor.results.last_moved_objects = public_zone_moves
                .iter()
                .map(|movement| movement.after)
                .collect();
            work.cursor.results.milled_mana_values = public_zone_moves
                .iter()
                .map(|movement| {
                    self.copiable_program(movement.after)
                        .ok_or(EffectError::InconsistentProgram)
                        .map(Program::mana_value)
                })
                .collect::<Result<Vec<_>, _>>()?;
        }
        let action = LibraryAction {
            kind: work.kind,
            player: work.player,
            requested: work.requested.clone(),
            looked_top_first: work.domain.clone(),
            top_first: top_first.clone(),
            bottom_top_first: bottom_top_first.clone(),
            selected_before_top_first: selected.clone(),
            public_zone_moves,
            graveyard_before_top_first: batch
                .logical
                .iter()
                .filter(|row| matches!(by_id[&row.object].after.zone(), ObjectZone::Graveyard(_)))
                .map(|row| row.object)
                .collect(),
        };
        self.emit_from(
            EventCause::Instruction {
                entry: work.cursor.entry,
                instruction: work.cursor.instruction,
            },
            &before,
            None,
            append,
            |previous, current, controllers| {
                let mut values = vec![RulesOccurrence::LibraryAction(action)];
                let reason = match work.kind {
                    LibraryActionKind::Surveil => ZoneReason::Surveil {
                        player: work.player,
                    },
                    LibraryActionKind::Mill => ZoneReason::Mill {
                        player: work.player,
                    },
                    LibraryActionKind::Scry => return Ok(values),
                };
                for row in &batch.logical {
                    values.push(zone_occurrence_at(
                        previous,
                        current,
                        by_id
                            .get(&row.object)
                            .ok_or(EffectError::InconsistentLibraryTopWork)?,
                        reason,
                        controllers,
                    )?);
                }
                Ok(values)
            },
        )
    }
}
fn validate_partition(
    domain: &[ObjectId],
    first: &[ObjectId],
    second: &[ObjectId],
) -> Result<(), EffectError> {
    let actual: BTreeSet<_> = first.iter().chain(second).copied().collect();
    let expected: BTreeSet<_> = domain.iter().copied().collect();
    if first.len().checked_add(second.len()) != Some(domain.len())
        || actual.len() != domain.len()
        || actual != expected
    {
        return Err(EffectError::InvalidLibraryPartition);
    }
    Ok(())
}
