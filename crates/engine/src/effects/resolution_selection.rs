//! One resolution cursor owns all affected-player choices, owner ordering and commit.
//! These native observations are privileged, including hidden hand domains.
use super::*;
use crate::occurrence::{EventCause, RulesOccurrence, ZoneReason};
use crate::program::{
    AffectedPlayers, ChosenHandAction, ObjectFilter, PermanentTypeFilter, PublicZoneDestination,
    ResolvingPlayer,
};
use crate::stack_window::ResolutionSelectionMove;
use crate::stack_window::StackEntry;
#[cfg(test)]
mod tests;

/// The kind of an executor-issued selected-object decision.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResolutionSelectionKind {
    VowCreature,
    ChooseCreatureToKeep,
    Bolster,
    Amass,
    Mill,
    LibraryToGraveyard,
    /// Choose zero or one creature card in your graveyard.
    GraveyardCreatureToHand,
    VoteForCard,
    ExileMilledCreatures,
    HandToBattlefield,
    GraveyardToBattlefield,
    ExileToBattlefield,
    TurnCreatureFaceUp,
    MeldPartner,
    Soulbond,
    EquipmentToToken,
    UnattachControlledEquipment,
    TokenReceivingEquipment,
    FoundSpellToCast,
    AttachAllEquipment,
    AttachChosenAurasAndEquipment,
    /// Place any of the selected hand or graveyard Auras attached to the source.
    AttachAurasFromHandOrGraveyard,
    CopyDifferentlyNamedTokens,
    /// CR 701.30a Populate: choose one controlled creature token to copy.
    Populate,
    Untap,
    /// An effect's selected objects would enter the battlefield.
    EnterBattlefield,
    /// An effect moves a selection to its owner’s library.
    ReturnToLibrary,
    ReturnToCommandZone,
    /// The whole cohort was determined by the effect. Only owner ordering is chosen.
    Destroy,
    Exile,
    /// A controlled permanent returns to its owner’s hand.
    ReturnToHand,
    /// A controlled current permanent moves to its owner's graveyard.
    Sacrifice,
    /// The controller may sacrifice an eligible creature or take the printed alternative.
    OptionalSacrifice,
    /// Select creatures to save by paying the same life amount for each.
    PayLifeToSaveCreatures,
    /// A physical card moves from its owner's hand to that graveyard.
    DiscardUpTo,
    Discard,
    /// Interactive Blight N: choose one controlled creature to receive N -1/-1 counters.
    BlightCreature,
    /// Interactive Collect Evidence N: choose graveyard cards whose total mana value is at least N.
    CollectEvidence,
    /// Interactive SelectZoneCardsToZone: choose filtered cards from a zone to move to a destination.
    SelectZoneCards,
    /// Universal non-target object selection with optional power constraints and keep-vs-rest action.
    ChooseObjects,
}

/// The original operation retained by the sole work owner.
// Held exclusively inside the single active resolution-selection frame.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolutionSelectionOperation {
    OwnedCommandersToHand,
    Bolster {
        amount: u16,
        eligible: BTreeSet<ObjectId>,
    },
    Amass {
        amount: u16,
        army: CreatureTypeId,
        subtype: CreatureTypeId,
        eligible: BTreeSet<ObjectId>,
    },
    ExileGraveyardTargets {
        objects: BTreeSet<ObjectId>,
    },
    ExileChosenGraveyardCards {
        player: SeatId,
        domain: BTreeSet<ObjectId>,
    },
    MillEachPlayer {
        amount: u16,
        segments: BTreeMap<SeatId, Vec<ObjectId>>,
    },
    LibraryCohortToGraveyard {
        player: SeatId,
        cards: BTreeSet<ObjectId>,
    },
    GraveyardCreatureToHand,
    ReturnGraveyardCardsToHand {
        objects: BTreeSet<ObjectId>,
    },
    RevealedHand {
        chooser: SeatId,
        minimum_mana_value: u16,
        nonland_only: bool,
        filter: Option<Box<ObjectFilter>>,
        count: std::num::NonZeroU8,
        action: ChosenHandAction,
    },
    AttachToSource {
        source: ObjectId,
        eligible: BTreeSet<ObjectId>,
        optional: bool,
    },
    EnterAurasAttachedToSource {
        source: ObjectId,
        eligible: BTreeSet<ObjectId>,
    },
    CopyDifferentlyNamedTokens,
    /// CR 701.30a Populate: choose one controlled creature token to copy.
    Populate,
    Untap {
        kind: CardType,
    },
    /// Actual bound permanent of a targeted destroy effect with applicable zone replacements.
    DestroySingle {
        object: ObjectId,
    },
    ReturnSingle {
        object: ObjectId,
    },
    /// A bound target exiled by a duration-bearing instruction.
    ExileSingle {
        object: ObjectId,
        source: Option<ObjectId>,
        monarch_controller: Option<SeatId>,
    },
    /// Automatically determined destruction cohort, including permanents owned by other players.
    ReturnAnnouncedTargets {
        objects: BTreeSet<ObjectId>,
    },
    ShuffleAnnouncedTargets {
        objects: BTreeSet<ObjectId>,
    },
    PutInLibrary {
        objects: BTreeSet<ObjectId>,
        placement: crate::program::LibraryPlacement,
    },
    PutHandCardOnBottom,
    ExileAnnouncedTargets {
        objects: BTreeSet<ObjectId>,
    },
    ExileLibrarySegment {
        player: SeatId,
        cards: Vec<ObjectId>,
    },
    ExileLinkedTargets {
        objects: BTreeSet<ObjectId>,
        source: ObjectId,
    },
    DestroyAnnouncedTargets {
        objects: BTreeSet<ObjectId>,
    },
    WholeGroup {
        group: ObjectFilter,
        destination: PublicZoneDestination,
    },
    /// Return current controlled permanents meeting the printed filter.
    ReturnToHand {
        filter: PermanentTypeFilter,
    },
    OtherControlledCreature {
        source: ObjectId,
        sacrifice: bool,
        or_exile_source: bool,
    },
    ExileChosen {
        filter: PermanentTypeFilter,
    },
    TapOrUntapControlled {
        tap: bool,
        filter: Box<ObjectFilter>,
    },
    ExileFromHand {
        allow_battlefield: bool,
    },
    /// The life price and accepted choices survive replacement and owner ordering.
    PayLifeToSaveCreatures {
        cost: RulesInteger,
        saved: BTreeMap<SeatId, Vec<ObjectId>>,
    },
    SacrificeContributor {
        eligible: BTreeSet<ObjectId>,
    },
    SacrificeColored,
    ChooseVow,
    ChooseCreaturesToKeep,
    SacrificeExceptVowed {
        kept: BTreeSet<ObjectId>,
    },
    /// Evaluated types filter a separately checked physical permanent domain.
    Sacrifice {
        filter: PermanentTypeFilter,
    },
    SacrificeMatching {
        filter: crate::program::SacrificeSelection,
        source: Option<ObjectId>,
    },
    /// The domain and commit both require physical-card origin and exact Hand.
    Discard,
    RandomDiscard {
        object: ObjectId,
    },
    RandomSacrifice {
        object: ObjectId,
    },
    Blight {
        amount: RulesInteger,
    },
    CollectEvidence {
        minimum: RulesInteger,
    },
    SelectZoneCards {
        source_zone: crate::program::SourceZoneFilter,
        filter: Box<ObjectFilter>,
        min: usize,
        destination: crate::program::SubjectDestination,
        reveal: bool,
        last_moved: BTreeSet<ObjectId>,
    },
    ChooseObjects {
        source_zone: crate::program::SourceZoneFilter,
        filter: Box<ObjectFilter>,
        min: usize,
        distinct_powers: bool,
        max_total_power: Option<RulesInteger>,
        unchosen_action: Option<crate::program::UnchosenObjectsAction>,
        last_moved: BTreeSet<ObjectId>,
    },
}
impl ResolutionSelectionOperation {
    fn is_sacrifice(&self) -> bool {
        matches!(
            self,
            Self::Sacrifice { .. }
                | Self::SacrificeMatching { .. }
                | Self::OtherControlledCreature {
                    sacrifice: true,
                    ..
                }
                | Self::SacrificeExceptVowed { .. }
                | Self::SacrificeColored
                | Self::SacrificeContributor { .. }
                | Self::RandomSacrifice { .. }
                | Self::PayLifeToSaveCreatures { .. }
        )
    }

    fn excluded_types(
        &self,
        cursor: &ResolutionCursor,
    ) -> Result<BTreeSet<CreatureTypeId>, EffectError> {
        match self {
            Self::WholeGroup { group, .. } => {
                if let Some(slot) = group.chosen_type_slot() {
                    cursor
                        .values
                        .get(&slot)
                        .map(|kind| BTreeSet::from([*kind]))
                        .ok_or(EffectError::InconsistentProgram)
                } else if group.uses_outside_named_slots() {
                    Ok(cursor.values.values().copied().collect())
                } else {
                    Ok(BTreeSet::new())
                }
            }
            _ => Ok(BTreeSet::new()),
        }
    }
    /// Observe the operation discriminant without supplying any work authority.
    pub fn kind(&self) -> ResolutionSelectionKind {
        match self {
            Self::ShuffleAnnouncedTargets { .. }
            | Self::PutInLibrary { .. }
            | Self::PutHandCardOnBottom => ResolutionSelectionKind::ReturnToLibrary,
            Self::MillEachPlayer { .. } => ResolutionSelectionKind::Mill,
            Self::LibraryCohortToGraveyard { .. } => ResolutionSelectionKind::LibraryToGraveyard,
            Self::OwnedCommandersToHand => ResolutionSelectionKind::ReturnToHand,
            Self::GraveyardCreatureToHand => ResolutionSelectionKind::GraveyardCreatureToHand,
            Self::ReturnGraveyardCardsToHand { .. } => ResolutionSelectionKind::ReturnToHand,
            Self::RevealedHand {
                action: ChosenHandAction::Discard,
                ..
            } => ResolutionSelectionKind::Discard,
            Self::RevealedHand {
                action: ChosenHandAction::Exile,
                ..
            } => ResolutionSelectionKind::Exile,
            Self::ExileGraveyardTargets { .. } | Self::ExileChosenGraveyardCards { .. } => {
                ResolutionSelectionKind::Exile
            }
            Self::EnterAurasAttachedToSource { .. } => {
                ResolutionSelectionKind::AttachAurasFromHandOrGraveyard
            }
            Self::AttachToSource { optional: true, .. } => {
                ResolutionSelectionKind::AttachChosenAurasAndEquipment
            }
            Self::AttachToSource {
                optional: false, ..
            } => ResolutionSelectionKind::AttachAllEquipment,
            Self::CopyDifferentlyNamedTokens => ResolutionSelectionKind::CopyDifferentlyNamedTokens,
            Self::Populate => ResolutionSelectionKind::Populate,
            Self::ChooseVow => ResolutionSelectionKind::VowCreature,
            Self::ChooseCreaturesToKeep => ResolutionSelectionKind::ChooseCreatureToKeep,
            Self::PayLifeToSaveCreatures { .. } => ResolutionSelectionKind::PayLifeToSaveCreatures,
            Self::Bolster { .. } => ResolutionSelectionKind::Bolster,
            Self::Amass { .. } => ResolutionSelectionKind::Amass,
            Self::Untap { .. } => ResolutionSelectionKind::Untap,
            Self::WholeGroup { destination, .. } => match destination {
                PublicZoneDestination::Hand => ResolutionSelectionKind::ReturnToHand,
                PublicZoneDestination::Exile => ResolutionSelectionKind::Exile,
                PublicZoneDestination::Graveyard => ResolutionSelectionKind::Destroy,
                PublicZoneDestination::Battlefield => ResolutionSelectionKind::EnterBattlefield,
                PublicZoneDestination::Command => ResolutionSelectionKind::ReturnToCommandZone,
                PublicZoneDestination::Library => ResolutionSelectionKind::ReturnToLibrary,
            },
            Self::ReturnAnnouncedTargets { .. } | Self::ReturnSingle { .. } => {
                ResolutionSelectionKind::ReturnToHand
            }
            Self::DestroyAnnouncedTargets { .. } | Self::DestroySingle { .. } => {
                ResolutionSelectionKind::Destroy
            }
            Self::ExileAnnouncedTargets { .. }
            | Self::ExileLibrarySegment { .. }
            | Self::ExileLinkedTargets { .. }
            | Self::ExileChosen { .. }
            | Self::ExileFromHand { .. }
            | Self::ExileSingle { .. } => ResolutionSelectionKind::Exile,
            Self::TapOrUntapControlled { .. } => ResolutionSelectionKind::Untap,
            Self::OtherControlledCreature {
                or_exile_source: true,
                ..
            } => ResolutionSelectionKind::OptionalSacrifice,
            Self::OtherControlledCreature { sacrifice, .. } => {
                if *sacrifice {
                    ResolutionSelectionKind::Sacrifice
                } else {
                    ResolutionSelectionKind::ReturnToHand
                }
            }
            Self::ReturnToHand { .. } => ResolutionSelectionKind::ReturnToHand,
            Self::SacrificeExceptVowed { .. }
            | Self::SacrificeContributor { .. }
            | Self::SacrificeColored
            | Self::Sacrifice { .. }
            | Self::SacrificeMatching { .. } => ResolutionSelectionKind::Sacrifice,
            Self::Discard | Self::RandomDiscard { .. } => ResolutionSelectionKind::Discard,
            Self::RandomSacrifice { .. } => ResolutionSelectionKind::Sacrifice,
            Self::Blight { .. } => ResolutionSelectionKind::BlightCreature,
            Self::CollectEvidence { .. } => ResolutionSelectionKind::CollectEvidence,
            Self::SelectZoneCards { .. } => ResolutionSelectionKind::SelectZoneCards,
            Self::ChooseObjects { .. } => ResolutionSelectionKind::ChooseObjects,
        }
    }
    fn movement(
        &self,
        selected: &ResolutionSelectedObject,
        destination: crate::program::PublicZoneDestination,
        origin: ObjectZone,
    ) -> Result<ResolutionSelectionMove, EffectError> {
        Ok(match self {
            Self::ExileFromHand { .. } => match origin {
                ObjectZone::Hand(player) if player == selected.player => {
                    ResolutionSelectionMove::FromHand {
                        player,
                        object: selected.object,
                        destination,
                    }
                }
                ObjectZone::Battlefield => ResolutionSelectionMove::BattlefieldEffect {
                    player: selected.player,
                    object: selected.object,
                    destination,
                },
                _ => return Err(EffectError::InconsistentSelectionWork),
            },
            Self::PutHandCardOnBottom => ResolutionSelectionMove::PutInLibrary {
                player: selected.player,
                object: selected.object,
                destination,
                placement: crate::program::LibraryPlacement::Bottom,
            },
            Self::PutInLibrary { placement, .. } => ResolutionSelectionMove::PutInLibrary {
                player: selected.player,
                object: selected.object,
                destination,
                placement: *placement,
            },
            Self::OwnedCommandersToHand => ResolutionSelectionMove::CommanderReturn {
                player: selected.player,
                object: selected.object,
                destination,
            },
            Self::GraveyardCreatureToHand
            | Self::ReturnGraveyardCardsToHand { .. }
            | Self::ExileGraveyardTargets { .. }
            | Self::ExileChosenGraveyardCards { .. } => ResolutionSelectionMove::GraveyardReturn {
                player: selected.player,
                object: selected.object,
                destination,
            },
            Self::ExileLibrarySegment { .. }
            | Self::MillEachPlayer { .. }
            | Self::LibraryCohortToGraveyard { .. } => ResolutionSelectionMove::LibraryEffect {
                player: selected.player,
                object: selected.object,
                destination,
            },
            Self::ChooseVow
            | Self::ChooseCreaturesToKeep
            | Self::AttachToSource { .. }
            | Self::EnterAurasAttachedToSource { .. }
            | Self::CopyDifferentlyNamedTokens
            | Self::Populate
            | Self::Bolster { .. }
            | Self::Amass { .. }
            | Self::Untap { .. }
            | Self::TapOrUntapControlled { .. }
            | Self::Blight { .. }
            | Self::CollectEvidence { .. }
            | Self::SelectZoneCards { .. }
            | Self::ChooseObjects { .. } => {
                return Err(EffectError::InconsistentSelectionWork);
            }
            Self::ShuffleAnnouncedTargets { .. }
            | Self::ExileAnnouncedTargets { .. }
            | Self::ReturnAnnouncedTargets { .. }
            | Self::ExileChosen { .. }
            | Self::ExileLinkedTargets { .. }
            | Self::ReturnSingle { .. }
            | Self::DestroyAnnouncedTargets { .. }
            | Self::DestroySingle { .. }
            | Self::ExileSingle { .. }
            | Self::WholeGroup { .. } => ResolutionSelectionMove::BattlefieldEffect {
                player: selected.player,
                object: selected.object,
                destination,
            },
            Self::OtherControlledCreature {
                sacrifice: false, ..
            }
            | Self::ReturnToHand { .. } => ResolutionSelectionMove::ReturnToHand {
                player: selected.player,
                object: selected.object,
                destination,
            },
            Self::OtherControlledCreature {
                sacrifice: true, ..
            }
            | Self::SacrificeExceptVowed { .. }
            | Self::SacrificeContributor { .. }
            | Self::SacrificeColored
            | Self::Sacrifice { .. }
            | Self::SacrificeMatching { .. }
            | Self::RandomSacrifice { .. }
            | Self::PayLifeToSaveCreatures { .. } => ResolutionSelectionMove::Sacrifice {
                player: selected.player,
                object: selected.object,
                destination,
            },
            Self::RevealedHand { .. } | Self::Discard | Self::RandomDiscard { .. } => {
                ResolutionSelectionMove::FromHand {
                    player: selected.player,
                    object: selected.object,
                    destination,
                }
            }
        })
    }
    pub(super) fn reason(&self, player: SeatId) -> Result<ZoneReason, EffectError> {
        Ok(match self {
            Self::ShuffleAnnouncedTargets { .. } => ZoneReason::ShufflePermanentIntoLibrary,
            Self::PutInLibrary { placement, .. } => ZoneReason::PutIntoOwnersLibrary(*placement),
            Self::PutHandCardOnBottom => {
                ZoneReason::PutIntoOwnersLibrary(crate::program::LibraryPlacement::Bottom)
            }
            Self::OwnedCommandersToHand
            | Self::GraveyardCreatureToHand
            | Self::ReturnGraveyardCardsToHand { .. } => ZoneReason::ReturnToOwnersHand,
            Self::ExileGraveyardTargets { .. } | Self::ExileChosenGraveyardCards { .. } => {
                ZoneReason::ExiledByEffect
            }
            Self::MillEachPlayer { .. } => ZoneReason::Mill { player },
            Self::LibraryCohortToGraveyard { .. } => ZoneReason::PutLibraryCohortIntoGraveyard,
            Self::ChooseVow
            | Self::ChooseCreaturesToKeep
            | Self::AttachToSource { .. }
            | Self::EnterAurasAttachedToSource { .. }
            | Self::CopyDifferentlyNamedTokens
            | Self::Populate
            | Self::Bolster { .. }
            | Self::Amass { .. }
            | Self::Untap { .. }
            | Self::TapOrUntapControlled { .. }
            | Self::Blight { .. }
            | Self::CollectEvidence { .. }
            | Self::SelectZoneCards { .. }
            | Self::ChooseObjects { .. } => {
                return Err(EffectError::InconsistentSelectionWork);
            }
            Self::WholeGroup { destination, .. } => match destination {
                PublicZoneDestination::Hand => ZoneReason::ReturnToOwnersHand,
                PublicZoneDestination::Exile => ZoneReason::ExiledByEffect,
                PublicZoneDestination::Graveyard => ZoneReason::Destroyed,
                PublicZoneDestination::Battlefield => ZoneReason::ReturnFromGraveyardToBattlefield,
                PublicZoneDestination::Command => {
                    return Err(EffectError::InconsistentSelectionWork);
                }
                PublicZoneDestination::Library => ZoneReason::ShuffleGraveyardIntoLibrary,
            },
            Self::ReturnAnnouncedTargets { .. } | Self::ReturnSingle { .. } => {
                ZoneReason::ReturnToOwnersHand
            }
            Self::DestroyAnnouncedTargets { .. } | Self::DestroySingle { .. } => {
                ZoneReason::Destroyed
            }
            Self::ExileAnnouncedTargets { .. }
            | Self::ExileLibrarySegment { .. }
            | Self::ExileLinkedTargets { .. }
            | Self::ExileChosen { .. }
            | Self::ExileFromHand { .. }
            | Self::ExileSingle { .. } => ZoneReason::ExiledByEffect,
            Self::OtherControlledCreature {
                sacrifice: false, ..
            }
            | Self::ReturnToHand { .. } => ZoneReason::ReturnToOwnersHand,
            Self::OtherControlledCreature {
                sacrifice: true, ..
            }
            | Self::SacrificeExceptVowed { .. }
            | Self::SacrificeContributor { .. }
            | Self::SacrificeColored
            | Self::Sacrifice { .. }
            | Self::SacrificeMatching { .. }
            | Self::RandomSacrifice { .. }
            | Self::PayLifeToSaveCreatures { .. } => ZoneReason::Sacrifice { player },
            Self::RevealedHand {
                action: ChosenHandAction::Exile,
                ..
            } => ZoneReason::ExiledByEffect,
            Self::RevealedHand {
                action: ChosenHandAction::Discard,
                ..
            }
            | Self::Discard
            | Self::RandomDiscard { .. } => ZoneReason::Discard { player },
        })
    }
}

/// A complete frozen current-incarnation domain, in actual affected-player order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolutionSelectionRequirement {
    /// Actual participant required to choose this row's objects.
    pub player: SeatId,
    /// Exact current identities frozen before any participant's selection.
    pub domain: Vec<ObjectId>,
    /// Nonnegative authored amount clamped to this domain before conversion.
    pub required: usize,
}
/// One accepted affected-player answer, retained before any physical movement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolutionAcceptedSelection {
    /// Actual affected chooser, independent of the selected cards' owners.
    pub player: SeatId,
    /// Distinct exact members of that player's frozen requirement.
    pub objects: Vec<ObjectId>,
}
/// Existing resolution-facing name for the neutral physical owner group.
pub type ResolutionGraveyardGroup = ZoneGraveyardGroup;
/// Existing resolution-facing name for a neutral owner permutation.
pub type ResolutionAcceptedGraveyardOrder = ZoneOwnerOrder;
/// A selected exact incarnation and the player performing this operation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolutionSelectedObject {
    /// Player sacrificing or discarding this object, not inferred from owner.
    pub player: SeatId,
    /// Actual before incarnation, never rebound through a later CardId lookup.
    pub object: ObjectId,
}
/// Complete internal batch. Application order differs from logical publication order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolutionSelectionBatch {
    /// Original affected-player choices, owned here rather than by the generic reducer.
    pub selections: Vec<ResolutionAcceptedSelection>,
    /// Complete settled replacement plan, absent for historical no-replacement work.
    pub replacements: Option<Box<ZoneReplacementBatch>>,
    /// Publication order: owner APNAP, chosen physical order, then token departures.
    pub logical: Vec<ResolutionSelectedObject>,
    /// Physical top-insertion order, reversing each owner's physical entrants.
    pub application: Vec<ResolutionSelectedObject>,
    /// Complete owner permutations, including internally determined singletons.
    pub owner_orders: Vec<ResolutionAcceptedGraveyardOrder>,
    /// Current Battlefield tokens selected for sacrifice; always empty for Discard.
    pub token_departures: Vec<ObjectId>,
}
/// Mutually exclusive owned stages, never a reply-supplied mutation plan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolutionSelectionStage {
    /// Every domain is frozen; accepted rows form the exact consumed APNAP prefix.
    Choosing {
        /// All affected players, including zero-count requirements.
        requirements: Vec<ResolutionSelectionRequirement>,
        /// First unconsumed requirement; equals accepted.len().
        next_player: usize,
        /// Previously accepted selections, including skipped zero-count rows.
        accepted: Vec<ResolutionAcceptedSelection>,
    },
    /// Complete precommit candidate reducer, retaining this owner's sole cursor.
    Replacing {
        /// Original selections retained by this resolution owner.
        selections: Vec<ResolutionAcceptedSelection>,
        /// Sole generic replacement reducer.
        batch: Box<ZoneReplacementBatch>,
    },
    /// All selections are complete before any owner ordering or physical move.
    Ordering {
        /// Complete affected-player selections retained in actual APNAP order.
        selections: Vec<ResolutionAcceptedSelection>,
        /// Settled final destinations/history; sources still belong to the before-world.
        replacements: Option<Box<ZoneReplacementBatch>>,
        /// Nonempty physical-card groups in owner APNAP order.
        groups: Vec<ResolutionGraveyardGroup>,
        /// First unordered owner group; equals orders.len().
        next_owner: usize,
        /// Accepted owner-order prefix, including internal singleton orders.
        orders: Vec<ResolutionAcceptedGraveyardOrder>,
    },
    /// Internal validated-shape batch awaiting one final revalidation and commit.
    Ready(ResolutionSelectionBatch),
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ResolutionSelectionWork {
    pub(super) cursor: ResolutionCursor,
    operation: ResolutionSelectionOperation,
    stage: ResolutionSelectionStage,
}
/// Borrowed complete state of the one owner. This is not a public seat-safe view.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResolutionSelectionState<'a> {
    /// Complete borrowed cursor, including body-local values.
    pub cursor: ResolutionState<'a>,
    /// The sole owner's original operation and exact filter.
    pub operation: &'a ResolutionSelectionOperation,
    /// Complete current stage and accepted prefixes; hidden data is privileged.
    pub stage: &'a ResolutionSelectionStage,
}

/// An exact immutable positive selection request issued by the executor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolutionSelectionFrame {
    revision: u64,
    entry: StackEntryId,
    instruction: usize,
    chooser: SeatId,
    operation: ResolutionSelectionKind,
    domain: Vec<ObjectId>,
    required: usize,
    custom_minimum: Option<usize>,
}
impl EffectWindow {
    /// Show only the already chosen battlefield permanents in this mixed-zone choice.
    pub fn prior_public_exile_choices(&self) -> impl Iterator<Item = (SeatId, ObjectId)> + '_ {
        self.resolution_selection_state()
            .into_iter()
            .filter(|state| {
                matches!(
                    state.operation,
                    ResolutionSelectionOperation::ExileFromHand {
                        allow_battlefield: true
                    }
                )
            })
            .filter_map(|state| match state.stage {
                ResolutionSelectionStage::Choosing { accepted, .. } => Some(accepted),
                _ => None,
            })
            .flatten()
            .flat_map(|row| {
                row.objects
                    .iter()
                    .copied()
                    .map(move |object| (row.player, object))
            })
            .filter(|&(_, object)| {
                self.window
                    .cards()
                    .get(object)
                    .is_some_and(|card| card.zone() == ObjectZone::Battlefield)
            })
    }

    /// Public selections already made during a vow or life-payment choice.
    pub fn prior_vow_choices(&self) -> Vec<(SeatId, ObjectId)> {
        let Some(PendingWork::ResolutionSelection(work)) = &self.pending_work else {
            return Vec::new();
        };
        let ResolutionSelectionStage::Choosing { accepted, .. } = &work.stage else {
            return Vec::new();
        };
        if !matches!(
            work.operation,
            ResolutionSelectionOperation::ChooseVow
                | ResolutionSelectionOperation::ChooseCreaturesToKeep
                | ResolutionSelectionOperation::PayLifeToSaveCreatures { .. }
        ) {
            return Vec::new();
        }
        accepted
            .iter()
            .flat_map(|row| row.objects.iter().map(move |&object| (row.player, object)))
            .collect()
    }
}
impl ResolutionSelectionFrame {
    pub(super) fn found_spell(
        revision: u64,
        entry: StackEntryId,
        instruction: usize,
        chooser: SeatId,
        domain: Vec<ObjectId>,
    ) -> Self {
        Self {
            revision,
            entry,
            instruction,
            chooser,
            domain,
            required: 1,
            operation: ResolutionSelectionKind::FoundSpellToCast,
            custom_minimum: None,
        }
    }
    pub(super) fn equipment(
        revision: u64,
        entry: StackEntryId,
        instruction: usize,
        chooser: SeatId,
        domain: Vec<ObjectId>,
        chosen: bool,
    ) -> Self {
        let operation = if chosen {
            ResolutionSelectionKind::TokenReceivingEquipment
        } else {
            ResolutionSelectionKind::EquipmentToToken
        };
        Self {
            revision,
            entry,
            instruction,
            chooser,
            domain,
            required: 1,
            operation,
            custom_minimum: None,
        }
    }
    pub(super) fn hand_entry(
        revision: u64,
        entry: StackEntryId,
        instruction: usize,
        chooser: SeatId,
        domain: Vec<ObjectId>,
        maximum: usize,
        operation: ResolutionSelectionKind,
    ) -> Self {
        let required = domain.len().min(maximum);
        Self {
            revision,
            entry,
            instruction,
            chooser,
            operation,
            domain,
            required,
            custom_minimum: None,
        }
    }
    /// Exact executor revision at which this immutable frame is issued.
    pub fn revision(&self) -> u64 {
        self.revision
    }
    /// Actual resolving entry whose captured body owns this work.
    pub fn entry(&self) -> StackEntryId {
        self.entry
    }
    /// Absolute authored instruction index retained by the sole cursor.
    pub fn instruction(&self) -> usize {
        self.instruction
    }
    /// Affected participant authorized to submit the selection.
    pub fn chooser(&self) -> SeatId {
        self.chooser
    }
    /// Operation kind; replies cannot change it or its retained filter.
    pub fn operation(&self) -> ResolutionSelectionKind {
        self.operation
    }
    /// Exact frozen current-incarnation domain, potentially private hand data.
    pub fn domain(&self) -> &[ObjectId] {
        &self.domain
    }
    /// Required distinct member count, clamped by the executor to domain size.
    pub fn minimum(&self) -> usize {
        if let Some(min) = self.custom_minimum {
            return min.min(self.required);
        }
        if self.operation == ResolutionSelectionKind::CollectEvidence {
            return usize::from(self.required > 0);
        }
        if matches!(
            self.operation,
            ResolutionSelectionKind::PayLifeToSaveCreatures
                | ResolutionSelectionKind::OptionalSacrifice
                | ResolutionSelectionKind::UnattachControlledEquipment
                | ResolutionSelectionKind::DiscardUpTo
                | ResolutionSelectionKind::ExileMilledCreatures
                | ResolutionSelectionKind::FoundSpellToCast
                | ResolutionSelectionKind::EquipmentToToken
                | ResolutionSelectionKind::TurnCreatureFaceUp
                | ResolutionSelectionKind::Soulbond
                | ResolutionSelectionKind::ExileToBattlefield
                | ResolutionSelectionKind::HandToBattlefield
                | ResolutionSelectionKind::AttachChosenAurasAndEquipment
                | ResolutionSelectionKind::AttachAurasFromHandOrGraveyard
                | ResolutionSelectionKind::CopyDifferentlyNamedTokens
                | ResolutionSelectionKind::Untap
                | ResolutionSelectionKind::GraveyardCreatureToHand
        ) {
            0
        } else {
            self.required
        }
    }
    pub fn required(&self) -> usize {
        self.required
    }
}
/// An exact immutable owner permutation request, distinct from a check decision.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolutionGraveyardOrderFrame {
    revision: u64,
    entry: StackEntryId,
    instruction: usize,
    chooser: SeatId,
    domain: Vec<ObjectId>,
    destination: PublicZoneDestination,
}
impl ResolutionGraveyardOrderFrame {
    pub(super) fn for_group(
        revision: u64,
        entry: StackEntryId,
        instruction: usize,
        group: &ResolutionGraveyardGroup,
    ) -> Self {
        Self {
            revision,
            entry,
            instruction,
            chooser: group.owner,
            domain: group.entrants.clone(),
            destination: group.destination,
        }
    }
    /// Exact executor revision required when resuming this owner choice.
    pub fn revision(&self) -> u64 {
        self.revision
    }
    /// Actual resolving entry, not a checks or cleanup authority.
    pub fn entry(&self) -> StackEntryId {
        self.entry
    }
    /// Absolute selected-object instruction retained by the cursor.
    pub fn instruction(&self) -> usize {
        self.instruction
    }
    /// Physical owner authorized to order all of these entrants.
    pub fn chooser(&self) -> SeatId {
        self.chooser
    }
    /// Zone into which the ordered physical cards will arrive.
    pub fn destination(&self) -> PublicZoneDestination {
        self.destination
    }
    /// Complete exact physical group, possibly selected by several controllers.
    pub fn domain(&self) -> &[ObjectId] {
        &self.domain
    }
}
impl ResolutionSelectionWork {
    fn frame(&self, revision: u64, _discard_up_to_ext: bool) -> Option<EffectDecision> {
        match &self.stage {
            ResolutionSelectionStage::Choosing {
                requirements,
                next_player,
                ..
            } => {
                let requirement = requirements.get(*next_player)?;
                (requirement.required > 0).then(|| {
                    EffectDecision::ResolutionSelect(ResolutionSelectionFrame {
                        revision,
                        entry: self.cursor.entry,
                        instruction: self.cursor.instruction,
                        chooser: match self.operation {
                            ResolutionSelectionOperation::RevealedHand { chooser, .. } => chooser,
                            _ => requirement.player,
                        },
                        operation: self.operation.kind(),
                        domain: requirement.domain.clone(),
                        required: requirement.required,
                        custom_minimum: match &self.operation {
                            ResolutionSelectionOperation::SelectZoneCards { min, .. }
                            | ResolutionSelectionOperation::ChooseObjects { min, .. } => Some(*min),
                            _ => None,
                        },
                    })
                })
            }
            ResolutionSelectionStage::Ordering {
                groups, next_owner, ..
            } => {
                let group = groups.get(*next_owner)?;
                (group.entrants.len() > 1).then(|| {
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
            ResolutionSelectionStage::Replacing { batch: work, .. } => {
                work.frame(revision).map(EffectDecision::ZoneReplacement)
            }
            ResolutionSelectionStage::Ready(_) => None,
        }
    }
}

impl EffectWindow {
    pub(super) fn selection_players(
        &self,
        players: AffectedPlayers,
        controller: SeatId,
        cursor: Option<&ResolutionCursor>,
    ) -> Result<Vec<SeatId>, EffectError> {
        match players {
            AffectedPlayers::One(role) => Ok(vec![self.resolving_player_at(role, cursor)?]),
            AffectedPlayers::EachPlayer => Ok(self.window.apnap_players()?.collect()),
            AffectedPlayers::EachOtherPlayer => Ok(self
                .window
                .apnap_players()?
                .filter(|seat| *seat != controller)
                .collect()),
            AffectedPlayers::EachOpponent => {
                let opponents = self
                    .combat_rules
                    .opponents(self.window.participants(), controller)?;
                Ok(self
                    .window
                    .apnap_players()?
                    .filter(|p| opponents.contains(p))
                    .collect())
            }
        }
    }

    /// Inspect every live requirement, accepted prefix and order without cloning a game.
    /// Hidden domains remain privileged; this accessor supplies no resume authority.
    pub fn resolution_selection_state(&self) -> Option<ResolutionSelectionState<'_>> {
        match self.pending_work.as_ref() {
            Some(PendingWork::ResolutionSelection(work)) => Some(ResolutionSelectionState {
                cursor: (&work.cursor).into(),
                operation: &work.operation,
                stage: &work.stage,
            }),
            None
            | Some(
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
                | PendingWork::Resolution(_)
                | PendingWork::OptionalEffect(_)
                | PendingWork::ResolutionPayment(_)
                | PendingWork::ManaColor(_)
                | PendingWork::TokenEntry(_)
                | PendingWork::Damage(_)
                | PendingWork::Checks(_)
                | PendingWork::Combat(_),
            ) => None,
        }
    }
    fn is_discard_up_to_ext(&self, cursor: &ResolutionCursor) -> bool {
        matches!(
            self.payloads
                .get(&cursor.entry)
                .and_then(StackPayload::instructions)
                .and_then(|ops| ops.get(cursor.instruction)),
            Some(Instruction::ZoneOp(_)) if false
        )
    }

    fn work_selection_frame(&self, work: &ResolutionSelectionWork) -> Option<EffectDecision> {
        work.frame(self.revision, self.is_discard_up_to_ext(&work.cursor))
    }

    /// Derive only the positive current request from the sole owned stage.
    pub(super) fn selection_frame(&self) -> Option<EffectDecision> {
        match self.pending_work.as_ref() {
            Some(PendingWork::ResolutionSelection(work)) => self.work_selection_frame(work),
            None
            | Some(
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
                | PendingWork::Resolution(_)
                | PendingWork::OptionalEffect(_)
                | PendingWork::ResolutionPayment(_)
                | PendingWork::ManaColor(_)
                | PendingWork::TokenEntry(_)
                | PendingWork::Damage(_)
                | PendingWork::Checks(_)
                | PendingWork::Combat(_),
            ) => None,
        }
    }

    pub(super) fn hand_selection_departure_admitted(
        &self,
        issued: Option<&EffectDecision>,
    ) -> bool {
        matches!(&self.pending_work, Some(PendingWork::ResolutionSelection(work))
            if matches!(work.operation, ResolutionSelectionOperation::Discard)
                && matches!(work.stage, ResolutionSelectionStage::Choosing { .. }))
            && issued.is_some()
            && self.selection_frame().as_ref() == issued
    }

    /// Discard cohorts use exact owner hands. A vanished hand retires only its own staged choice.
    pub(super) fn refresh_hand_selection_after_departure(&mut self) -> Result<(), EffectError> {
        let (filter_clone, check_rows) = {
            let Some(PendingWork::ResolutionSelection(work)) = &self.pending_work else {
                return Err(EffectError::WrongSelectionFrame);
            };
            let ResolutionSelectionStage::Choosing {
                requirements,
                next_player,
                accepted,
            } = &work.stage
            else {
                return Err(EffectError::WrongPhase);
            };
            if *next_player != accepted.len() || *next_player > requirements.len() {
                return Err(EffectError::InconsistentSelectionWork);
            }
            let participants = self.window.participants();
            let rows = requirements
                .iter()
                .filter(|r| participants.contains(&r.player))
                .map(|r| (r.player, r.domain.clone()))
                .collect::<Vec<_>>();
            let filter_clone: Option<Box<crate::program::ObjectFilter>> = None;
            (filter_clone, rows)
        };
        let evaluated = if filter_clone.is_some() {
            Some(self.evaluated_characteristics()?)
        } else {
            None
        };
        let ability_source =
            self.window
                .resolving()
                .and_then(|entry| match entry.kind() {
                    StackKind::ActivatedAbility { source }
                    | StackKind::TriggeredAbility { source } => Some(source.id()),
                    StackKind::CardSpell(id) => Some(*id),
                    _ => None,
                });
        let empty_excluded = BTreeSet::new();
        for (player, domain) in &check_rows {
            let hand = self
                .window
                .cards()
                .iter()
                .filter(|object| {
                    object.card_id().is_some()
                        && object.zone() == ObjectZone::Hand(*player)
                        && match (&filter_clone, &evaluated) {
                            (Some(filter), Some(eval)) => eval.get(object.id()).is_some_and(|v| {
                                self.matches_object_filter(
                                    **filter,
                                    v,
                                    *player,
                                    ability_source,
                                    &empty_excluded,
                                    eval,
                                )
                            }),
                            _ => true,
                        }
                })
                .map(GameObject::id);
            if !hand.eq(domain.iter().copied()) {
                return Err(EffectError::WrongPhase);
            }
        }
        let participants = self.window.participants().to_vec();
        let Some(PendingWork::ResolutionSelection(work)) = &mut self.pending_work else {
            return Err(EffectError::WrongSelectionFrame);
        };
        let ResolutionSelectionStage::Choosing {
            requirements,
            next_player,
            accepted,
        } = &mut work.stage
        else {
            return Err(EffectError::WrongPhase);
        };
        requirements.retain(|requirement| participants.contains(&requirement.player));
        accepted.retain(|selection| participants.contains(&selection.player));
        *next_player = accepted.len();
        Ok(())
    }

    /// Freeze every affected domain before transferring the existing cursor.
    ///
    /// # Errors
    /// Rejects missing resolving ownership, an invalid captured player/amount,
    /// inconsistent bounded conversion or a failed characteristic evaluation.
    fn source_sacrifice_player(&self, entry: &StackEntry, instruction: &Instruction) -> SeatId {
        if matches!(instruction, Instruction::SacrificeEvoked)
            && let StackKind::TriggeredAbility { source } = entry.kind()
            && let Some(player) = self
                .window
                .cards()
                .get(source.id())
                .and_then(GameObject::controller)
        {
            return player;
        }
        entry.controller()
    }
    pub(super) fn opposing_sacrifice_prohibited(
        &self,
        evaluated: &EvaluatedCharacteristics,
        player: SeatId,
        effect_controller: SeatId,
    ) -> Result<bool, EffectError> {
        Ok(evaluated.ignores_opposing_sacrifices(player)
            && self
                .combat_rules
                .opponents(self.window.participants(), player)?
                .contains(&effect_controller))
    }

    /// Send an already-chosen group through ordinary sacrifice replacement and ordering.
    pub(super) fn begin_fixed_sacrifice(
        &mut self,
        controller: SeatId,
        player: SeatId,
        subjects: &[ObjectId],
    ) -> Result<(), EffectError> {
        let evaluated = self.evaluated_characteristics()?;
        let protected = self.opposing_sacrifice_prohibited(&evaluated, player, controller)?;
        let objects: Vec<_> = subjects
            .iter()
            .copied()
            .filter(|_| !protected)
            .filter(|id| {
                !evaluated.sacrifice_prohibited(*id)
                    && evaluated.get(*id).is_some_and(|o| {
                        o.zone == ObjectZone::Battlefield && o.controller == Some(player)
                    })
            })
            .collect();
        let Some(PendingWork::Resolution(cursor)) = self.pending_work.take() else {
            return Err(EffectError::WrongResolutionFrame);
        };
        self.pending_work = Some(PendingWork::ResolutionSelection(Box::new(
            ResolutionSelectionWork {
                cursor,
                operation: ResolutionSelectionOperation::Sacrifice {
                    filter: PermanentTypeFilter {
                        any_types: Default::default(),
                        required_types: BTreeSet::new(),
                        forbidden_types: BTreeSet::new(),
                        object_filter: None,
                    },
                },
                stage: ResolutionSelectionStage::Choosing {
                    requirements: vec![ResolutionSelectionRequirement {
                        player,
                        required: objects.len(),
                        domain: objects.clone(),
                    }],
                    next_player: 1,
                    accepted: vec![ResolutionAcceptedSelection { player, objects }],
                },
            },
        )));
        Ok(())
    }

    pub(super) fn begin_resolution_selection(
        &mut self,
        instruction: &Instruction,
        append: &mut RulesJournal,
    ) -> Result<bool, EffectError> {
        if self.requires_source_sacrifice(instruction)? {
            let entry = self
                .window
                .resolving()
                .ok_or(EffectError::WrongResolutionFrame)?;
            let source_id = match entry.kind() {
                StackKind::TriggeredAbility { source } | StackKind::ActivatedAbility { source } => {
                    Some(source.id())
                }
                StackKind::DelayedCopy { .. }
                    if matches!(instruction, Instruction::SacrificeDelayedObjects(_)) =>
                {
                    None
                }
                _ => return Err(EffectError::InconsistentProgram),
            };
            let subjects = match instruction {
                Instruction::SacrificeDelayedObjects(ids) => ids.as_slice(),
                Instruction::EndStepCast { permanent, .. } => std::slice::from_ref(permanent),
                _ => std::slice::from_ref(
                    source_id.as_ref().ok_or(EffectError::InconsistentProgram)?,
                ),
            };
            let player = self.source_sacrifice_player(entry, instruction);
            self.begin_fixed_sacrifice(entry.controller(), player, subjects)?;
            return Ok(true);
        }
        // A mass destruction effect chooses no objects. Feed its full cohort into
        // the same replacement, physical-owner ordering and simultaneous commit.
        let all;
        let (players, amount, operation) = match instruction {
            Instruction::ChooseRevealedHandCard {
                minimum_mana_value,
                nonland_only,
                filter,
                count,
                action,
            } => {
                let entry = self
                    .window
                    .resolving()
                    .ok_or(EffectError::WrongResolutionFrame)?;
                let target = self.resolving_player(ResolvingPlayer::TargetPlayer)?;
                let hand: Vec<_> = self
                    .window
                    .cards()
                    .iter()
                    .filter(|object| object.zone() == ObjectZone::Hand(target))
                    .cloned()
                    .map(|object| RulesOccurrence::HandCardRevealed { object })
                    .collect();
                let chooser = entry.controller();
                let cause = EventCause::Instruction {
                    entry: entry.id(),
                    instruction: self
                        .resolution_cursor()
                        .ok_or(EffectError::WrongResolutionFrame)?
                        .instruction,
                };
                if !hand.is_empty() {
                    let before = self.rules_snapshot(None)?;
                    self.emit_from(cause, &before, None, append, |_, _, _| Ok(hand))?;
                }
                all = ValueExpr::Literal(i64::from(count.get()).into());
                (
                    AffectedPlayers::One(ResolvingPlayer::TargetPlayer),
                    &all,
                    ResolutionSelectionOperation::RevealedHand {
                        chooser,
                        minimum_mana_value: *minimum_mana_value,
                        nonland_only: *nonland_only,
                        filter: filter.clone(),
                        count: *count,
                        action: *action,
                    },
                )
            }
            Instruction::ReturnDelayedPermanentToHand(object) => {
                all = ValueExpr::Literal(1.into());
                (
                    AffectedPlayers::EachPlayer,
                    &all,
                    ResolutionSelectionOperation::ReturnSingle { object: *object },
                )
            }
            Instruction::ExileDefeatedSiege => {
                let entry = self
                    .window
                    .resolving()
                    .ok_or(EffectError::WrongResolutionFrame)?;
                let (StackKind::TriggeredAbility { source }
                | StackKind::ActivatedAbility { source }) = entry.kind()
                else {
                    return Err(EffectError::InconsistentProgram);
                };
                all = ValueExpr::Literal(1.into());
                (
                    AffectedPlayers::EachPlayer,
                    &all,
                    ResolutionSelectionOperation::ExileSingle {
                        object: source.id(),
                        source: None,
                        monarch_controller: None,
                    },
                )
            }
            Instruction::ExileDelayedObjects(ids) => {
                let objects: BTreeSet<_> = ids
                    .iter()
                    .copied()
                    .filter(|id| {
                        self.window
                            .cards()
                            .get(*id)
                            .is_some_and(|p| p.zone() == ObjectZone::Battlefield)
                    })
                    .collect();
                all = ValueExpr::Literal(RulesInteger::from_usize(objects.len()));
                (
                    AffectedPlayers::EachPlayer,
                    &all,
                    ResolutionSelectionOperation::ExileAnnouncedTargets { objects },
                )
            }
            Instruction::AffectPermanentTargets {
                action:
                    action @ (crate::program::PermanentTargetAction::ReturnToHand
                    | crate::program::PermanentTargetAction::Destroy
                    | crate::program::PermanentTargetAction::Exile),
                ..
            } => {
                let objects = self.legal_permanent_targets()?;
                all = ValueExpr::Literal(RulesInteger::from_usize(objects.len()));
                let operation = match action {
                    crate::program::PermanentTargetAction::Destroy => {
                        ResolutionSelectionOperation::DestroyAnnouncedTargets { objects }
                    }
                    crate::program::PermanentTargetAction::Exile => {
                        ResolutionSelectionOperation::ExileAnnouncedTargets { objects }
                    }
                    _ => ResolutionSelectionOperation::ReturnAnnouncedTargets { objects },
                };
                (AffectedPlayers::EachPlayer, &all, operation)
            }
            Instruction::AffectPermanentTargets { .. } => return Ok(false),
            Instruction::ExileOnePerPlayerUntilSourceLeaves => {
                let entry = self
                    .window
                    .resolving()
                    .ok_or(EffectError::WrongResolutionFrame)?;
                let source = self
                    .execution_bindings(entry.id())
                    .and_then(|b| b.trigger.as_ref())
                    .ok_or(EffectError::InconsistentProgram)?
                    .source
                    .object
                    .id();
                if self
                    .window
                    .cards()
                    .get(source)
                    .is_none_or(|p| p.zone() != ObjectZone::Battlefield)
                {
                    self.advance_instruction()?;
                    return Ok(true);
                }
                let objects = self.legal_permanent_targets()?;
                all = ValueExpr::Literal(RulesInteger::from_usize(objects.len()));
                (
                    AffectedPlayers::EachPlayer,
                    &all,
                    ResolutionSelectionOperation::ExileLinkedTargets { objects, source },
                )
            }
            Instruction::Populate => {
                all = ValueExpr::Literal(1.into());
                (
                    AffectedPlayers::One(crate::program::ResolvingPlayer::Controller),
                    &all,
                    ResolutionSelectionOperation::Populate,
                )
            }
            Instruction::Amass {
                army,
                subtype,
                amount,
            } => {
                let entry = self
                    .window
                    .resolving()
                    .ok_or(EffectError::WrongResolutionFrame)?;
                let eligible = self.controlled_armies(entry.controller(), *army)?;
                all = ValueExpr::Literal(1.into());
                (
                    AffectedPlayers::One(crate::program::ResolvingPlayer::Controller),
                    &all,
                    ResolutionSelectionOperation::Amass {
                        amount: *amount,
                        army: *army,
                        subtype: *subtype,
                        eligible,
                    },
                )
            }
            Instruction::Bolster(amount) => {
                let current = self.evaluated_characteristics()?;
                let controller = self.resolving_player(ResolvingPlayer::Controller)?;
                let controlled = current.objects.values().filter(|p| {
                    p.zone == ObjectZone::Battlefield
                        && p.controller == Some(controller)
                        && p.is_creature()
                });
                let min = controlled
                    .clone()
                    .filter_map(|p| p.power_toughness.as_ref().map(|pt| &pt.derived.toughness))
                    .min();
                let eligible = controlled
                    .filter(|p| {
                        p.power_toughness
                            .as_ref()
                            .is_some_and(|pt| Some(&pt.derived.toughness) == min)
                    })
                    .map(|p| p.object)
                    .collect();
                all = ValueExpr::Literal(1.into());
                (
                    AffectedPlayers::One(crate::program::ResolvingPlayer::Controller),
                    &all,
                    ResolutionSelectionOperation::Bolster {
                        amount: *amount,
                        eligible,
                    },
                )
            }
            Instruction::Sacrifice {
                players,
                amount,
                filter,
            } => (
                *players,
                amount,
                ResolutionSelectionOperation::Sacrifice {
                    filter: filter.clone(),
                },
            ),
            Instruction::SacrificeRandomUntilLand => {
                let object = self
                    .resolution_cursor()
                    .and_then(|cursor| cursor.random_selection)
                    .ok_or(EffectError::WrongResolutionFrame)?;
                let (role, operation) = (
                    ResolvingPlayer::TargetPlayer,
                    ResolutionSelectionOperation::RandomSacrifice { object },
                );
                all = ValueExpr::Literal(1.into());
                (AffectedPlayers::One(role), &all, operation)
            }
            Instruction::Discard { players, amount } => {
                (*players, amount, ResolutionSelectionOperation::Discard)
            }
            Instruction::Blight { player, amount } => {
                let entry = self
                    .window
                    .resolving()
                    .ok_or(EffectError::WrongResolutionFrame)?;
                let count = self
                    .resolve_value_expr(entry.controller(), amount)?
                    .max(0.into());
                all = ValueExpr::Literal(1.into());
                (
                    AffectedPlayers::One(*player),
                    &all,
                    ResolutionSelectionOperation::Blight { amount: count },
                )
            }
            Instruction::CollectEvidence { player, minimum } => {
                let entry = self
                    .window
                    .resolving()
                    .ok_or(EffectError::WrongResolutionFrame)?;
                let needed = self
                    .resolve_value_expr(entry.controller(), minimum)?
                    .max(0.into());
                all = ValueExpr::Literal(RulesInteger::from_usize(
                    self.window.cards().iter().count(),
                ));
                (
                    AffectedPlayers::One(*player),
                    &all,
                    ResolutionSelectionOperation::CollectEvidence { minimum: needed },
                )
            }
            Instruction::ChooseObjects {
                players,
                source_zone,
                filter,
                min,
                max,
                distinct_powers,
                max_total_power,
                unchosen_action,
            } => {
                let entry = self
                    .window
                    .resolving()
                    .ok_or(EffectError::WrongResolutionFrame)?;
                let min_val = self
                    .resolve_value_expr(entry.controller(), min)?
                    .max(0.into())
                    .to_usize()
                    .unwrap_or(0);
                let max_power = match max_total_power {
                    Some(expr) => Some(self.resolve_value_expr(entry.controller(), expr)?),
                    None => None,
                };
                let last_moved = if *source_zone == crate::program::SourceZoneFilter::LastMoved {
                    self.resolution_cursor()
                        .map(|cursor| cursor.results.last_moved_objects.iter().copied().collect())
                        .unwrap_or_default()
                } else {
                    BTreeSet::new()
                };
                (
                    *players,
                    max,
                    ResolutionSelectionOperation::ChooseObjects {
                        source_zone: *source_zone,
                        filter: filter.clone(),
                        min: min_val,
                        distinct_powers: *distinct_powers,
                        max_total_power: max_power,
                        unchosen_action: *unchosen_action,
                        last_moved,
                    },
                )
            }
            Instruction::BranchIfFalse { .. }
            | Instruction::Jump { .. }
            | Instruction::OptionalSequence { .. }
            | Instruction::OptionalEffect { .. }
            | Instruction::OfferDynamicManaPayment { .. }
            | Instruction::ChooseOption { .. }
            | Instruction::ExileTopPlay { .. }
            | Instruction::DivideDamageExprAmongTargets { .. }
            | Instruction::BecomeCopy { .. }
            | Instruction::DistributeCountersAmongTargets { .. }
            | Instruction::RollDie { .. }
            | Instruction::FlipCoins { .. }
            | Instruction::ChooseValue { .. }
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
            | Instruction::Ripple(_)
            | Instruction::Demonstrate
            | Instruction::Enlist
            | Instruction::ChooseOneLastMoved { .. }
            | Instruction::ZoneOp(_)
            | Instruction::ApplyContinuous(_)
            | Instruction::PromptChoice(_)
            | Instruction::BranchCondition { .. }
            | Instruction::ForEach { .. }
            | Instruction::ForEachPlayerQuery { .. }
            | Instruction::BindEnv { .. }
            | Instruction::SetObjectTag { .. }
            | Instruction::UpdateStateTrack { .. }
            | Instruction::SideDeckOp(_)
            | Instruction::StackOp(_)
            | Instruction::BindResolutionX(_)
            | Instruction::CreateAuraTokenAttached { .. }
            | Instruction::ExploreSubject(_)
            | Instruction::ScheduleDelayedEventTrigger { .. }
            | Instruction::RepeatLoop { .. }
            | Instruction::ResolvePowerAbility(_)
            | Instruction::CreateTokenAndAttach { .. }
            | Instruction::CreateTokens { .. }
            | Instruction::ChooseCreatureType { .. }
            | Instruction::DamageFromPower(_)
            | Instruction::CreatureDealsPowerDamageToOther { .. }
            | Instruction::RememberPermanentTarget
            | Instruction::AdditionalCombat { .. }
            | Instruction::Proliferate
            | Instruction::DamageTargetControllersCreatures { .. }
            | Instruction::CreateDamageMultiplier { .. }
            | Instruction::OfferSourceCounters { .. }
            | Instruction::ResolveKeywordTrigger(_)
            | Instruction::ReturnChosenXGraveyardCreatures
            | Instruction::DivideThreeDamageAmongTargets
            | Instruction::DamageOrderedTargets { .. }
            | Instruction::FixedTargetClause(_)
            | Instruction::ReturnGraveyardCreatureWithCorpse(_)
            | Instruction::PutHandPermanentsOntoBattlefield { .. }
            | Instruction::MayRevealLibraryTop
            | Instruction::ManifestTop { .. }
            | Instruction::ChooseFoundSpell
            | Instruction::CastFoundSpell(_)
            | Instruction::SearchLibrary { .. }
            | Instruction::RequireCondition(_)
            | Instruction::Encore
            | Instruction::ReselectAttackDefender
            | Instruction::SetBlockDeclarations(_)
            | Instruction::AdditionalLoyaltyActivation
            | Instruction::ReturnExiledTargetCohort { .. }
            | Instruction::ReturnCardsDestroyedThisWay
            | Instruction::ModifyAnyNumberOfPermanents { .. }
            | Instruction::PairWithCreature { .. }
            | Instruction::ScheduleDelayedStepTrigger { .. }
            | Instruction::EmitReflexiveTrigger { .. }
            | Instruction::Ninjutsu
            | Instruction::Unearth
            | Instruction::Cascade
            | Instruction::CascadeBottom
            | Instruction::Myriad
            | Instruction::WatchNextSpellOrLoyaltyTwice
            | Instruction::WatchNextLoyaltyCopy
            | Instruction::CopyCapturedAbility
            | Instruction::CopyOnCast(_)
            | Instruction::MadnessCast
            | Instruction::MiracleCast(_)
            | Instruction::CastDefeatedSiege
            | Instruction::ReboundCast(_)
            | Instruction::EndStepCast { .. }
            | Instruction::SacrificeDelayedObjects(_)
            | Instruction::ScheduleArtifactReturn(_)
            | Instruction::ReturnDelayedGraveyardCard(_)
            | Instruction::ReturnDelayedExileWithCounters(_)
            | Instruction::ReturnDelayedExile(_)
            | Instruction::ExileUnearthed(_)
            | Instruction::ResolveTransfer(_)
            | Instruction::ExtortDrain
            | Instruction::ResolveGrowth(_)
            | Instruction::ResolveCounterKeyword(_)
            | Instruction::SacrificeEvoked
            | Instruction::SacrificeStateTriggerSource
            | Instruction::Attach(_)
            | Instruction::AdvanceClassLevel(_)
            | Instruction::SolveSourceCase
            | Instruction::BecomePreparedSource
            | Instruction::ReturnDelayedGraveyardToHand(_)
            | Instruction::DamageFromGrantingPermanent { .. }
            | Instruction::Scry { .. }
            | Instruction::Surveil { .. }
            | Instruction::Mill { .. }
            | Instruction::OfferAnyPlayer { .. }
            | Instruction::DealDamageToAnnouncedTargets { .. }
            | Instruction::GrantChosenProtection { .. }
            | Instruction::LookTopChooseExt { .. }
            | Instruction::RevealTopTwoPiles { .. }
            | Instruction::RevealUntilMatchExt { .. }
            | Instruction::CreateTokenCopyExt { .. }
            | Instruction::Earthbend { .. }
            | Instruction::ReturnEarthbentCard
            | Instruction::AddManaForPlayer { .. }
            | Instruction::SelectExtremeLifePlayer { .. }
            | Instruction::DrawCards(_)
            | Instruction::MutateObject(_)
            | Instruction::AdjustLife(_)
            | Instruction::DealDamage(_)
            | Instruction::MutatePlayer(_) => return Ok(false),
        };
        let entry = self
            .window
            .resolving()
            .ok_or(EffectError::WrongResolutionFrame)?;
        let ability_source = match entry.kind() {
            StackKind::ActivatedAbility { source } | StackKind::TriggeredAbility { source } => {
                Some(source.id())
            }
            StackKind::CardSpell(id) => Some(*id),
            _ => None,
        };
        let amount = self.resolve_value_expr(entry.controller(), amount)?;
        if amount < 0.into() {
            return Err(EffectError::InconsistentSelectionWork);
        }
        let players = self.selection_players(players, entry.controller(), None)?;
        let excluded = operation.excluded_types(
            self.resolution_cursor()
                .ok_or(EffectError::WrongResolutionFrame)?,
        )?;
        let evaluated = self.evaluated_characteristics()?;
        let mut requirements = Vec::with_capacity(players.len());
        for player in players {
            if let ResolutionSelectionOperation::PayLifeToSaveCreatures { cost, .. } = &operation {
                let domain = self.creatures_controllable_for_life_choice(&evaluated, player);
                let total = self
                    .life
                    .get(player)
                    .ok_or(EffectError::InconsistentSelectionWork)?;
                let required = (0..domain.len())
                    .take_while(|index| {
                        !cost.is_positive()
                            || &cost.times(&RulesInteger::from_usize(index + 1)) <= total
                    })
                    .count();
                requirements.push(ResolutionSelectionRequirement {
                    player,
                    domain,
                    required,
                });
                continue;
            }
            let protected = operation.is_sacrifice()
                && self.opposing_sacrifice_prohibited(&evaluated, player, entry.controller())?;
            let domain: Vec<_> = self
                .window
                .cards()
                .iter()
                .filter(|_| !protected)
                .filter(|object| {
                    self.selection_eligible(
                        &operation,
                        player,
                        object,
                        ability_source,
                        &evaluated,
                        &excluded,
                    )
                })
                .map(GameObject::id)
                .collect();
            let bound = RulesInteger::from_usize(domain.len());
            let mut required = std::cmp::min(&amount, &bound)
                .to_usize()
                .ok_or(EffectError::InconsistentSelectionWork)?;
            match &operation {
                ResolutionSelectionOperation::CollectEvidence { minimum } => {
                    let total_mv = domain.iter().fold(RulesInteger::from(0), |acc, id| {
                        acc.plus(&self.permanent_mana_value(*id).unwrap_or_else(|| 0.into()))
                    });
                    if !minimum.is_positive() || total_mv < *minimum {
                        required = 0;
                    }
                }
                ResolutionSelectionOperation::SelectZoneCards { min, .. }
                    if domain.len() < *min =>
                {
                    required = 0;
                }
                ResolutionSelectionOperation::ChooseObjects { .. } if domain.is_empty() => {
                    required = 0;
                }
                _ => {}
            }
            requirements.push(ResolutionSelectionRequirement {
                player,
                domain,
                required,
            });
        }
        let Some(PendingWork::Resolution(cursor)) = self.pending_work.take() else {
            return Err(EffectError::WrongResolutionFrame);
        };
        self.pending_work = Some(PendingWork::ResolutionSelection(Box::new(
            ResolutionSelectionWork {
                cursor,
                operation,
                stage: ResolutionSelectionStage::Choosing {
                    requirements,
                    next_player: 0,
                    accepted: vec![],
                },
            },
        )));
        Ok(true)
    }

    /// Move resolved `objects` to a concrete `SubjectDestination` (used by both `MoveSubject`
    /// and `SelectZoneCardsToZone`). Caller must resolve `ChoiceTopOrBottom` beforehand.
    pub(super) fn move_objects_to_subject_destination(
        &mut self,
        entry: &StackEntry,
        instruction: usize,
        actor: SeatId,
        objects: &[ObjectId],
        destination: crate::program::SubjectDestination,
        append: &mut RulesJournal,
    ) -> Result<usize, EffectError> {
        Ok(self
            .move_objects_to_subject_destination_ids(
                entry,
                instruction,
                actor,
                objects,
                destination,
                append,
            )?
            .len())
    }

    /// Move resolved `objects` to `destination` and return the post-move `ObjectId`s
    /// (CR 400.7) so subsequent `LastMoved` instructions resolve the new incarnations.
    pub(super) fn move_objects_to_subject_destination_ids(
        &mut self,
        entry: &StackEntry,
        instruction: usize,
        actor: SeatId,
        objects: &[ObjectId],
        destination: crate::program::SubjectDestination,
        append: &mut RulesJournal,
    ) -> Result<Vec<ObjectId>, EffectError> {
        use crate::program::{DelayedReturnTiming, LibraryPlacement, SubjectDestination};
        let mut live: Vec<ObjectId> = objects
            .iter()
            .copied()
            .filter(|id| self.window.cards().get(*id).is_some())
            .collect();
        if matches!(
            destination,
            SubjectDestination::Library(LibraryPlacement::BottomRandom)
        ) {
            let mut seen = BTreeSet::new();
            live.retain(|id| seen.insert(*id));
        } else {
            live.sort();
            live.dedup();
        }
        if live.is_empty() {
            return Ok(Vec::new());
        }
        let source_id = match entry.kind() {
            StackKind::ActivatedAbility { source } | StackKind::TriggeredAbility { source } => {
                Some(source.id())
            }
            StackKind::CardSpell(id) => Some(*id),
            _ => None,
        };
        if matches!(
            destination,
            SubjectDestination::Exile {
                delayed_return: Some(DelayedReturnTiming::UntilSourceLeaves),
            }
        ) && !source_id.is_some_and(|src| {
            self.window
                .cards()
                .get(src)
                .is_some_and(|card| card.zone() == ObjectZone::Battlefield)
        }) {
            return Ok(Vec::new());
        }
        let (mut moves, reason): (Vec<(ObjectId, ObjectDestination)>, ZoneReason) =
            match destination {
                SubjectDestination::Battlefield {
                    under_owner_control,
                    ..
                } => {
                    let mut m = Vec::with_capacity(live.len());
                    for id in &live {
                        let owner = self
                            .window
                            .cards()
                            .get(*id)
                            .ok_or(EffectError::UnknownObject)?
                            .owner();
                        let controller = if under_owner_control { owner } else { actor };
                        m.push((*id, ObjectDestination::Battlefield { controller }));
                    }
                    (m, ZoneReason::ReturnFromGraveyardToBattlefield)
                }
                SubjectDestination::Hand => (
                    live.iter()
                        .map(|id| (*id, ObjectDestination::Hand))
                        .collect(),
                    ZoneReason::ReturnToOwnersHand,
                ),
                SubjectDestination::Graveyard => (
                    live.iter()
                        .map(|id| (*id, ObjectDestination::Graveyard))
                        .collect(),
                    ZoneReason::PutLibraryCohortIntoGraveyard,
                ),
                SubjectDestination::Exile { .. } => (
                    live.iter()
                        .map(|id| (*id, ObjectDestination::Exile))
                        .collect(),
                    ZoneReason::ExiledByEffect,
                ),
                SubjectDestination::Library(placement) => {
                    let dest = match placement {
                        LibraryPlacement::Top => ObjectDestination::LibraryTop,
                        LibraryPlacement::Bottom | LibraryPlacement::BottomRandom => {
                            ObjectDestination::LibraryBottom
                        }
                        LibraryPlacement::FromTop(offset) => {
                            ObjectDestination::LibraryFromTop(offset)
                        }
                        LibraryPlacement::ChoiceTopOrBottom { .. } => {
                            return Err(EffectError::InconsistentProgram);
                        }
                    };
                    (
                        live.iter().map(|id| (*id, dest)).collect(),
                        ZoneReason::PutIntoOwnersLibrary(placement),
                    )
                }
            };
        for (obj_id, dest) in &mut moves {
            if !matches!(dest, ObjectDestination::Exile)
                && self
                    .window
                    .cards()
                    .get(*obj_id)
                    .is_some_and(|c| c.zone() == ObjectZone::Battlefield)
                && self.temporary_rules.iter().any(|active| {
                    matches!(
                        active.rule,
                        crate::program::TemporaryRuleKind::ExileIfLeavesBattlefield { .. }
                    ) && active.resolved_objects.contains(obj_id)
                })
            {
                *dest = ObjectDestination::Exile;
            }
        }
        let mut replacement_batch = None;
        if matches!(destination, SubjectDestination::Battlefield { .. }) {
            let mut entering_controllers = BTreeMap::new();
            let mut intents = Vec::new();
            for &(obj_id, dest) in &moves {
                if let ObjectDestination::Battlefield { controller } = dest {
                    entering_controllers.insert(obj_id, controller);
                    intents.push(zone_replacement::ZoneMoveIntent {
                        before: obj_id,
                        reason,
                    });
                }
            }
            if !intents.is_empty() {
                let mut batch = self.begin_zone_replacements_with_entry(
                    zone_replacement::AuthorizedZoneBatch {
                        cause: EventCause::Instruction {
                            entry: entry.id(),
                            instruction,
                        },
                        action: crate::program::ZoneActionKind::EffectEnterBattlefield,
                        intents,
                    },
                    Some(&entering_controllers),
                )?;
                if batch.progress(self.revision)?.is_none() {
                    for (obj_id, dest) in &mut moves {
                        if let Ok(replaced) = batch.destination(*obj_id) {
                            match replaced {
                                PublicZoneDestination::Battlefield
                                | PublicZoneDestination::Command => {}
                                PublicZoneDestination::Exile => *dest = ObjectDestination::Exile,
                                PublicZoneDestination::Graveyard => {
                                    *dest = ObjectDestination::Graveyard;
                                }
                                PublicZoneDestination::Hand => *dest = ObjectDestination::Hand,
                                PublicZoneDestination::Library => {
                                    *dest = ObjectDestination::LibraryTop;
                                }
                            }
                        }
                    }
                    replacement_batch = Some(batch);
                }
            }
        }
        let before = self.rules_snapshot(None)?;
        let (window, mut movements) = self.window.record_linked_return_moves(&moves)?;
        self.window = window;
        let new_ids: Vec<ObjectId> = movements.iter().map(|m| m.after.id()).collect();
        if let SubjectDestination::Battlefield {
            tapped,
            attacking,
            transformed,
            with_counter,
            attach_to_self,
            ..
        } = destination
        {
            for m in &mut movements {
                if m.after.zone() != ObjectZone::Battlefield {
                    continue;
                }
                let new_id = m.after.id();
                self.characteristics.register_battlefield_entry(new_id)?;
                if tapped && let Ok(updated) = self.window.enter_tapped(new_id) {
                    m.after = updated;
                }
                if transformed
                    && self.window.transform(new_id).is_ok()
                    && let Some(updated) = self.window.cards().get(new_id).cloned()
                {
                    m.after = updated;
                }
                if let Some((ck, cnt)) = with_counter
                    && cnt > 0
                {
                    let amount = RulesInteger::from_usize(usize::from(cnt));
                    match ck {
                        CounterKind::PlusOne => {
                            let _ = self.characteristics.place_counters(
                                new_id,
                                &crate::characteristics::PowerToughnessCounters {
                                    plus_one: amount,
                                    minus_one: 0.into(),
                                },
                            );
                        }
                        CounterKind::MinusOne => {
                            let _ = self.characteristics.place_counters(
                                new_id,
                                &crate::characteristics::PowerToughnessCounters {
                                    plus_one: 0.into(),
                                    minus_one: amount,
                                },
                            );
                        }
                        other => {
                            if let Some(map) =
                                self.characteristics.named_permanent_counters_mut(other)
                            {
                                let cur = map.get(&new_id).cloned().unwrap_or_else(|| 0.into());
                                map.insert(new_id, cur.plus(&amount));
                            }
                        }
                    }
                }
                if attach_to_self
                    && let Some(src) = source_id
                    && self
                        .window
                        .cards()
                        .get(src)
                        .is_some_and(|c| c.zone() == ObjectZone::Battlefield)
                {
                    let _ = self
                        .characteristics
                        .attach(new_id, TargetRef::Object(src), false);
                }
                if attacking
                    && self.combat.is_some()
                    && self
                        .window
                        .cards()
                        .get(new_id)
                        .and_then(GameObject::controller)
                        == Some(self.window.active_player())
                    && let Ok(current_eval) = self.evaluated_characteristics()
                    && let Ok(defenders) =
                        self.attack_destinations(self.window.active_player(), &current_eval)
                    && let Some((&target, &seat)) = defenders.first_key_value()
                {
                    let _ = self.put_attacking_at(new_id, seat, target);
                }
            }
        }
        self.emit_from(
            EventCause::Instruction {
                entry: entry.id(),
                instruction,
            },
            &before,
            None,
            append,
            |previous, current, controllers| {
                movements
                    .iter()
                    .map(|m| zone_occurrence_at(previous, current, m, reason, controllers))
                    .collect()
            },
        )?;
        if let Some(batch) = replacement_batch.as_ref() {
            self.apply_zone_replacement_extras_from_movements(
                Some(batch),
                &movements,
                EventCause::Instruction {
                    entry: entry.id(),
                    instruction,
                },
                append,
            )?;
        }
        if let SubjectDestination::Exile {
            delayed_return: Some(timing),
        } = destination
            && !new_ids.is_empty()
        {
            match timing {
                DelayedReturnTiming::Immediately => {
                    self.begin_linked_cohort_return(
                        LinkedReturnOrigin::ChosenExile {
                            controller: None,
                            tapped: false,
                            entry: entry.id(),
                            instruction,
                        },
                        new_ids.clone(),
                        append,
                    )?;
                }
                DelayedReturnTiming::NextEndStep => {
                    let snapshot = self.rules_snapshot(None)?;
                    let appearance = source_id
                        .and_then(|sid| {
                            snapshot
                                .view()
                                .appearance(sid)
                                .or_else(|| self.departed_reflexive_sources.get(&sid).cloned())
                        })
                        .or_else(|| {
                            self.execution_bindings(entry.id())
                                .and_then(|b| b.trigger.as_ref())
                                .map(|t| t.source.clone())
                        });
                    if let Some(app) = appearance {
                        self.triggers.delay_step_body(
                            app,
                            entry.controller(),
                            crate::turn::Step::End,
                            false,
                            Box::new(ExecutableBody::new(
                                BodyTarget::None,
                                vec![Instruction::ReturnDelayedExile(new_ids.clone())],
                            )),
                        );
                    }
                }
                DelayedReturnTiming::UntilSourceLeaves => {
                    if let Some(src) = source_id {
                        self.linked_exiles
                            .entry(src)
                            .or_default()
                            .extend(new_ids.iter().copied());
                    }
                }
                DelayedReturnTiming::LinkedGroup(group) => {
                    if let Some(src) = source_id {
                        self.independent_exile_links
                            .entry((src, group))
                            .or_default()
                            .extend(new_ids.iter().copied());
                    }
                }
            }
        }
        Ok(new_ids)
    }
    /// Advance only this instruction's owned stages. No player choice changes cards.
    ///
    /// # Errors
    /// Rejects an absent/inconsistent owner or stage and propagates final batch,
    /// physical movement, publisher and cursor failures. Call only on an owned
    /// tentative successor; no failed prefix is an adoptable game result.
    pub(super) fn progress_resolution_selection(
        &mut self,
        append: &mut RulesJournal,
    ) -> Result<Option<EffectDecision>, EffectError> {
        let Some(PendingWork::ResolutionSelection(work)) = self.pending_work.take() else {
            return Err(EffectError::WrongSelectionFrame);
        };
        let ResolutionSelectionWork {
            mut cursor,
            mut operation,
            mut stage,
        } = *work;
        let discard_entire_hand = matches!(
            self.payloads
                .get(&cursor.entry)
                .and_then(StackPayload::instructions)
                .and_then(|ops| ops.get(cursor.instruction)),
            Some(Instruction::Discard {
                amount: ValueExpr::PlayerStat(crate::program::PlayerStatExpr::LargestHandSize),
                ..
            })
        );
        loop {
            stage = match stage {
                ResolutionSelectionStage::Choosing {
                    requirements,
                    mut next_player,
                    mut accepted,
                } => {
                    if accepted.len() != next_player || next_player > requirements.len() {
                        return Err(EffectError::InconsistentSelectionWork);
                    }
                    while let Some(requirement) = requirements.get(next_player) {
                        let forced_army = matches!(
                            operation,
                            ResolutionSelectionOperation::Amass { .. }
                                | ResolutionSelectionOperation::ChooseCreaturesToKeep
                                | ResolutionSelectionOperation::Blight { .. }
                                | ResolutionSelectionOperation::Populate
                        ) && requirement.domain.len() == 1;
                        let forced_select_all = matches!(
                            operation,
                            ResolutionSelectionOperation::SelectZoneCards { min, .. }
                                if min == requirement.required && requirement.required == requirement.domain.len()
                        ) || matches!(
                            operation,
                            ResolutionSelectionOperation::ChooseObjects {
                                min,
                                distinct_powers: false,
                                max_total_power: None,
                                ..
                            } if min == requirement.required && requirement.required == requirement.domain.len()
                        );
                        let forced_graveyard_cards = matches!(
                            operation,
                            ResolutionSelectionOperation::ExileChosenGraveyardCards { .. }
                        ) && requirement.required
                            == requirement.domain.len();
                        if discard_entire_hand
                            || forced_army
                            || forced_select_all
                            || forced_graveyard_cards
                            || matches!(
                                operation,
                                ResolutionSelectionOperation::RandomDiscard { .. }
                                    | ResolutionSelectionOperation::RandomSacrifice { .. }
                                    | ResolutionSelectionOperation::ReturnGraveyardCardsToHand { .. }
                                    | ResolutionSelectionOperation::SacrificeExceptVowed { .. }
                                    | ResolutionSelectionOperation::OwnedCommandersToHand
                                    | ResolutionSelectionOperation::ExileGraveyardTargets { .. }
                                    | ResolutionSelectionOperation::SacrificeColored
                                    | ResolutionSelectionOperation::ExileLibrarySegment { .. }
                                    | ResolutionSelectionOperation::ShuffleAnnouncedTargets { .. }
                                    | ResolutionSelectionOperation::PutInLibrary { .. }
                                    | ResolutionSelectionOperation::ExileAnnouncedTargets { .. }
                                    | ResolutionSelectionOperation::MillEachPlayer { .. }
                                    | ResolutionSelectionOperation::LibraryCohortToGraveyard { .. }
                                    | ResolutionSelectionOperation::ExileLinkedTargets { .. }
                                    | ResolutionSelectionOperation::ReturnAnnouncedTargets { .. }
                                    | ResolutionSelectionOperation::ReturnSingle { .. }
                                    | ResolutionSelectionOperation::DestroyAnnouncedTargets { .. }
                                    | ResolutionSelectionOperation::DestroySingle { .. }
                                    | ResolutionSelectionOperation::ExileSingle { .. }
                                    | ResolutionSelectionOperation::WholeGroup { .. }
                            )
                        {
                            accepted.push(ResolutionAcceptedSelection {
                                player: requirement.player,
                                objects: requirement.domain.clone(),
                            });
                            next_player += 1;
                            continue;
                        }
                        if requirement.required > 0 {
                            break;
                        }
                        accepted.push(ResolutionAcceptedSelection {
                            player: requirement.player,
                            objects: vec![],
                        });
                        next_player += 1;
                    }
                    if next_player < requirements.len() {
                        let work = ResolutionSelectionWork {
                            cursor,
                            operation,
                            stage: ResolutionSelectionStage::Choosing {
                                requirements,
                                next_player,
                                accepted,
                            },
                        };
                        let frame = self
                            .work_selection_frame(&work)
                            .ok_or(EffectError::InconsistentSelectionWork)?;
                        self.pending_work = Some(PendingWork::ResolutionSelection(Box::new(work)));
                        return Ok(Some(frame));
                    }
                    if operation == ResolutionSelectionOperation::ChooseCreaturesToKeep {
                        cursor.vowed_creatures.extend(
                            accepted
                                .iter()
                                .flat_map(|selection| selection.objects.iter().copied()),
                        );
                        self.pending_work = Some(PendingWork::Resolution(cursor));
                        self.advance_instruction()?;
                        return Ok(None);
                    }
                    if operation == ResolutionSelectionOperation::ChooseVow {
                        let controller = self
                            .window
                            .resolving()
                            .ok_or(EffectError::WrongResolutionFrame)?
                            .controller();
                        let before = self.rules_snapshot(None)?;
                        let mut changes = Vec::new();
                        for selection in &accepted {
                            for &object in &selection.objects {
                                let added = super::permanent_counters::effect_counter_multiplier(
                                    &before.view().evaluated,
                                    object,
                                );
                                let old = self.counters_of_kind(object, CounterKind::Vow);
                                let after = old.plus(&added);
                                self.characteristics
                                    .vow_counters
                                    .insert(object, after.clone());
                                self.characteristics
                                    .vow_protected_players
                                    .entry(object)
                                    .or_default()
                                    .insert(controller);
                                cursor.vowed_creatures.insert(object);
                                changes.push(RulesOccurrence::PermanentCounterChanged {
                                    object,
                                    kind: CounterKind::Vow,
                                    before: old,
                                    after,
                                });
                            }
                        }
                        if !changes.is_empty() {
                            self.emit_from(
                                EventCause::Instruction {
                                    entry: cursor.entry,
                                    instruction: cursor.instruction,
                                },
                                &before,
                                None,
                                append,
                                |_, _, _| Ok(changes),
                            )?;
                        }
                        self.pending_work = Some(PendingWork::Resolution(cursor));
                        self.advance_instruction()?;
                        return Ok(None);
                    }
                    if let ResolutionSelectionOperation::EnterAurasAttachedToSource {
                        source, ..
                    } = operation
                    {
                        let [selection] = accepted.as_slice() else {
                            return Err(EffectError::InconsistentSelectionWork);
                        };
                        let origin = LinkedReturnOrigin::HandOrGraveyardAuras {
                            player: selection.player,
                            host: source,
                            entry: cursor.entry,
                            instruction: cursor.instruction,
                        };
                        self.pending_work = Some(PendingWork::Resolution(cursor));
                        self.begin_linked_cohort_return(origin, selection.objects.clone(), append)?;
                        self.advance_instruction()?;
                        return Ok(None);
                    }
                    if let ResolutionSelectionOperation::AttachToSource { source, .. } = operation {
                        let [selection] = accepted.as_slice() else {
                            return Err(EffectError::InconsistentSelectionWork);
                        };
                        let before = self.rules_snapshot(None)?;
                        let current = before.view().evaluated;
                        let mut changed = Vec::new();
                        for id in &selection.objects {
                            if let Some(event) = self.attach_at_cut(
                                cursor.entry,
                                *id,
                                TargetRef::Object(source),
                                &current,
                            )? {
                                changed.push(event);
                            }
                        }
                        let changed_count = changed.len();
                        if !changed.is_empty() {
                            self.emit_from(
                                EventCause::Instruction {
                                    entry: cursor.entry,
                                    instruction: cursor.instruction,
                                },
                                &before,
                                None,
                                append,
                                |_, _, _| Ok(changed),
                            )?;
                        }
                        cursor.results.last_action_succeeded = changed_count > 0;
                        cursor.results.last_action_count =
                            Some(RulesInteger::from_usize(changed_count));
                        self.pending_work = Some(PendingWork::Resolution(cursor));
                        self.advance_instruction()?;
                        return Ok(None);
                    }
                    if matches!(
                        operation,
                        ResolutionSelectionOperation::CopyDifferentlyNamedTokens
                            | ResolutionSelectionOperation::Populate
                    ) {
                        let [selection] = accepted.as_slice() else {
                            return Err(EffectError::InconsistentSelectionWork);
                        };
                        let groups = selection
                            .objects
                            .iter()
                            .map(|id| {
                                let program = self
                                    .copiable_program(*id)
                                    .ok_or(EffectError::InconsistentSelectionWork)?;
                                Ok((selection.player, 1, program.clone(), false))
                            })
                            .collect::<Result<Vec<_>, EffectError>>()?;
                        self.pending_work = Some(PendingWork::Resolution(cursor));
                        self.begin_materialized_token_entries(groups, None, false)?;
                        return Ok(None);
                    }
                    if let ResolutionSelectionOperation::Amass {
                        amount,
                        army,
                        subtype,
                        ref eligible,
                    } = operation
                    {
                        let [selection] = accepted.as_slice() else {
                            return Err(EffectError::InconsistentSelectionWork);
                        };
                        if selection.objects.len() != usize::from(!eligible.is_empty()) {
                            return Err(EffectError::InconsistentSelectionWork);
                        }
                        if let [object] = selection.objects.as_slice() {
                            self.grow_army(
                                &cursor,
                                selection.player,
                                *object,
                                [army, subtype],
                                amount,
                                append,
                            )?;
                            cursor.results.last_amassed_army = Some(*object);
                        }
                        self.pending_work = Some(PendingWork::Resolution(cursor));
                        self.advance_instruction()?;
                        return Ok(None);
                    }
                    if let ResolutionSelectionOperation::Bolster {
                        amount,
                        ref eligible,
                    } = operation
                    {
                        let [selection] = accepted.as_slice() else {
                            return Err(EffectError::InconsistentSelectionWork);
                        };
                        if selection.objects.len() != usize::from(!eligible.is_empty())
                            || selection.objects.iter().any(|id| !eligible.contains(id))
                        {
                            return Err(EffectError::InconsistentSelectionWork);
                        }
                        self.place_plus_one_counter_amount(
                            selection.objects.to_vec(),
                            i64::from(amount).into(),
                            EventCause::Instruction {
                                entry: cursor.entry,
                                instruction: cursor.instruction,
                            },
                            append,
                        )?;
                        self.pending_work = Some(PendingWork::Resolution(cursor));
                        self.advance_instruction()?;
                        return Ok(None);
                    }
                    if let ResolutionSelectionOperation::Untap { .. } = operation {
                        return Err(EffectError::InconsistentProgram);
                    }
                    if let ResolutionSelectionOperation::TapOrUntapControlled { tap, .. } =
                        operation
                    {
                        let before = self.rules_snapshot(None)?;
                        let mut changed = Vec::new();
                        for row in &accepted {
                            for id in &row.objects {
                                if self.window.cards().get(*id).and_then(GameObject::tapped)
                                    == Some(!tap)
                                {
                                    changed.push(*id);
                                }
                            }
                        }
                        if !changed.is_empty() {
                            let occurrences = if tap {
                                let (window, changes) =
                                    self.window.tap_for_effect(cursor.entry, &changed)?;
                                self.window = window;
                                occurrence_flow::status_occurrences(
                                    changes,
                                    crate::occurrence::StatusReason::Effect,
                                )
                            } else {
                                self.untap_for_effect_with_stun(cursor.entry, &changed)?
                            };
                            self.emit_from(
                                EventCause::Instruction {
                                    entry: cursor.entry,
                                    instruction: cursor.instruction,
                                },
                                &before,
                                None,
                                append,
                                |_, _, _| Ok(occurrences),
                            )?;
                            let eval_lki = self.evaluated_characteristics()?;
                            cursor.results.last_action_lki = changed
                                .iter()
                                .filter_map(|id| eval_lki.get(*id).cloned())
                                .collect();
                            cursor.results.last_action_succeeded = true;
                            cursor.results.last_action_count =
                                Some(RulesInteger::from_usize(changed.len()));
                        } else {
                            cursor.results.last_action_lki.clear();
                            cursor.results.last_action_succeeded = false;
                            cursor.results.last_action_count = Some(0.into());
                        }
                        self.pending_work = Some(PendingWork::Resolution(cursor));
                        self.advance_instruction()?;
                        return Ok(None);
                    }
                    if let ResolutionSelectionOperation::Blight { amount } = operation {
                        let [selection] = accepted.as_slice() else {
                            return Err(EffectError::InconsistentSelectionWork);
                        };
                        if let Some(&target_id) = selection.objects.first() {
                            if amount.is_positive() {
                                let before_snapshot = self.rules_snapshot(None)?;
                                let before = self.characteristics.counters(target_id);
                                let delta = crate::characteristics::PowerToughnessCounters {
                                    plus_one: 0.into(),
                                    minus_one: amount,
                                };
                                self.characteristics.place_counters(target_id, &delta)?;
                                let after = self.characteristics.counters(target_id);
                                self.emit_from(
                                    EventCause::Instruction {
                                        entry: cursor.entry,
                                        instruction: cursor.instruction,
                                    },
                                    &before_snapshot,
                                    None,
                                    append,
                                    |previous, current, _| {
                                        Ok(vec![RulesOccurrence::CountersPlaced {
                                            object: target_id,
                                            before: before.clone(),
                                            added: delta.clone(),
                                            after: after.clone(),
                                            before_appearance: Box::new(
                                                previous
                                                    .appearance(target_id)
                                                    .ok_or(EffectError::InconsistentProgram)?,
                                            ),
                                            after_appearance: Box::new(
                                                current
                                                    .appearance(target_id)
                                                    .ok_or(EffectError::InconsistentProgram)?,
                                            ),
                                        }])
                                    },
                                )?;
                            }
                            cursor.results.last_action_succeeded = true;
                            cursor.results.last_action_count = Some(1.into());
                        } else {
                            cursor.results.last_action_succeeded = false;
                            cursor.results.last_action_count = Some(0.into());
                        }
                        self.pending_work = Some(PendingWork::Resolution(cursor));
                        self.advance_instruction()?;
                        return Ok(None);
                    }
                    if let ResolutionSelectionOperation::CollectEvidence { minimum } = operation {
                        let [selection] = accepted.as_slice() else {
                            return Err(EffectError::InconsistentSelectionWork);
                        };
                        let succeeded = if !minimum.is_positive() {
                            true
                        } else if selection.objects.is_empty() {
                            false
                        } else {
                            let before = self.rules_snapshot(None)?;
                            let moves: Vec<_> = selection
                                .objects
                                .iter()
                                .map(|id| ResolutionSelectionMove::GraveyardReturn {
                                    player: selection.player,
                                    object: *id,
                                    destination: PublicZoneDestination::Exile,
                                })
                                .collect();
                            let (window, movements) =
                                self.window.record_resolution_selection_moves(
                                    self.window.revision(),
                                    cursor.entry,
                                    moves,
                                )?;
                            self.window = window;
                            self.emit_from(
                                EventCause::Instruction {
                                    entry: cursor.entry,
                                    instruction: cursor.instruction,
                                },
                                &before,
                                None,
                                append,
                                |previous, current, controllers| {
                                    movements
                                        .iter()
                                        .map(|m| {
                                            zone_occurrence_at(
                                                previous,
                                                current,
                                                m,
                                                ZoneReason::ExiledByEffect,
                                                controllers,
                                            )
                                        })
                                        .collect()
                                },
                            )?;
                            true
                        };
                        cursor.results.last_action_succeeded = succeeded;
                        cursor.results.last_action_count =
                            Some(RulesInteger::from_usize(selection.objects.len()));
                        self.pending_work = Some(PendingWork::Resolution(cursor));
                        self.advance_instruction()?;
                        return Ok(None);
                    }
                    if let ResolutionSelectionOperation::SelectZoneCards { destination, .. } =
                        operation
                    {
                        let [selection] = accepted.as_slice() else {
                            return Err(EffectError::InconsistentSelectionWork);
                        };
                        let entry = self
                            .window
                            .resolving()
                            .cloned()
                            .ok_or(EffectError::WrongResolutionFrame)?;
                        let moved_ids = if selection.objects.is_empty() {
                            Vec::new()
                        } else {
                            self.move_objects_to_subject_destination_ids(
                                &entry,
                                cursor.instruction,
                                selection.player,
                                &selection.objects,
                                destination,
                                append,
                            )?
                        };
                        cursor.results.last_action_succeeded = !moved_ids.is_empty();
                        cursor.results.last_action_count =
                            Some(RulesInteger::from_usize(moved_ids.len()));
                        cursor.results.last_moved_objects = moved_ids;
                        self.pending_work = Some(PendingWork::Resolution(cursor));
                        self.advance_instruction()?;
                        return Ok(None);
                    }
                    if let ResolutionSelectionOperation::ChooseObjects {
                        source_zone,
                        ref filter,
                        unchosen_action,
                        ref last_moved,
                        ..
                    } = operation
                    {
                        let entry = self
                            .window
                            .resolving()
                            .cloned()
                            .ok_or(EffectError::WrongResolutionFrame)?;
                        let chosen_ids: Vec<ObjectId> = accepted
                            .iter()
                            .flat_map(|row| row.objects.iter().copied())
                            .collect();
                        let chosen_set: BTreeSet<ObjectId> = chosen_ids.iter().copied().collect();
                        cursor.vowed_creatures.extend(chosen_ids.iter().copied());
                        let eval_lki = self.evaluated_characteristics()?;
                        cursor.results.last_action_lki = chosen_ids
                            .iter()
                            .filter_map(|id| eval_lki.get(*id).cloned())
                            .collect();
                        cursor.results.last_moved_objects = chosen_ids.clone();
                        cursor.results.last_action_succeeded = !chosen_ids.is_empty();
                        cursor.results.last_action_count =
                            Some(RulesInteger::from_usize(chosen_ids.len()));
                        if let Some(action) = unchosen_action {
                            let evaluated = self.evaluated_characteristics()?;
                            let ability_source = match entry.kind() {
                                StackKind::ActivatedAbility { source }
                                | StackKind::TriggeredAbility { source } => Some(source.id()),
                                StackKind::CardSpell(id) => Some(*id),
                                _ => None,
                            };
                            let mut unchosen = Vec::new();
                            let mut seen = BTreeSet::new();
                            let excluded = BTreeSet::new();
                            let probe_op = ResolutionSelectionOperation::ChooseObjects {
                                source_zone,
                                filter: filter.clone(),
                                min: 0,
                                distinct_powers: false,
                                max_total_power: None,
                                unchosen_action: None,
                                last_moved: last_moved.clone(),
                            };
                            for row in &accepted {
                                for object in self.window.cards().iter() {
                                    let oid = object.id();
                                    if chosen_set.contains(&oid) || !seen.insert(oid) {
                                        continue;
                                    }
                                    if self.selection_eligible(
                                        &probe_op,
                                        row.player,
                                        object,
                                        ability_source,
                                        &evaluated,
                                        &excluded,
                                    ) {
                                        unchosen.push((row.player, oid));
                                    }
                                }
                            }
                            if !unchosen.is_empty() {
                                match action {
                                    crate::program::UnchosenObjectsAction::Tap => {
                                        let to_tap: Vec<ObjectId> = unchosen
                                            .iter()
                                            .map(|(_, id)| *id)
                                            .filter(|id| {
                                                self.window
                                                    .cards()
                                                    .get(*id)
                                                    .and_then(GameObject::tapped)
                                                    == Some(false)
                                            })
                                            .collect();
                                        if !to_tap.is_empty() {
                                            let before = self.rules_snapshot(None)?;
                                            let (window, changes) = self
                                                .window
                                                .tap_for_effect(cursor.entry, &to_tap)?;
                                            self.window = window;
                                            let occs = occurrence_flow::status_occurrences(
                                                changes,
                                                crate::occurrence::StatusReason::Effect,
                                            );
                                            self.emit_from(
                                                EventCause::Instruction {
                                                    entry: cursor.entry,
                                                    instruction: cursor.instruction,
                                                },
                                                &before,
                                                None,
                                                append,
                                                |_, _, _| Ok(occs),
                                            )?;
                                        }
                                    }
                                    crate::program::UnchosenObjectsAction::Destroy => {
                                        let dest_ids: Vec<ObjectId> = unchosen
                                            .iter()
                                            .map(|(_, id)| *id)
                                            .filter(|id| {
                                                evaluated.get(*id).is_some_and(|v| {
                                                    !v.has_keyword(KeywordAbility::Indestructible)
                                                })
                                            })
                                            .collect();
                                        if !dest_ids.is_empty() {
                                            let _ = self.move_objects_to_subject_destination_ids(
                                                &entry,
                                                cursor.instruction,
                                                entry.controller(),
                                                &dest_ids,
                                                crate::program::SubjectDestination::Graveyard,
                                                append,
                                            )?;
                                        }
                                    }
                                    crate::program::UnchosenObjectsAction::Exile => {
                                        let ids: Vec<ObjectId> =
                                            unchosen.iter().map(|(_, id)| *id).collect();
                                        let _ = self.move_objects_to_subject_destination_ids(
                                            &entry,
                                            cursor.instruction,
                                            entry.controller(),
                                            &ids,
                                            crate::program::SubjectDestination::Exile {
                                                delayed_return: None,
                                            },
                                            append,
                                        )?;
                                    }
                                    crate::program::UnchosenObjectsAction::ReturnToHand => {
                                        let ids: Vec<ObjectId> =
                                            unchosen.iter().map(|(_, id)| *id).collect();
                                        let _ = self.move_objects_to_subject_destination_ids(
                                            &entry,
                                            cursor.instruction,
                                            entry.controller(),
                                            &ids,
                                            crate::program::SubjectDestination::Hand,
                                            append,
                                        )?;
                                    }
                                    crate::program::UnchosenObjectsAction::Sacrifice
                                    | crate::program::UnchosenObjectsAction::Discard => {
                                        let ids: Vec<ObjectId> =
                                            unchosen.iter().map(|(_, id)| *id).collect();
                                        let _ = self.move_objects_to_subject_destination_ids(
                                            &entry,
                                            cursor.instruction,
                                            entry.controller(),
                                            &ids,
                                            crate::program::SubjectDestination::Graveyard,
                                            append,
                                        )?;
                                    }
                                }
                            }
                        }
                        self.pending_work = Some(PendingWork::Resolution(cursor));
                        self.advance_instruction()?;
                        return Ok(None);
                    }
                    if let ResolutionSelectionOperation::PayLifeToSaveCreatures { saved, .. } =
                        &mut operation
                    {
                        *saved = accepted
                            .into_iter()
                            .map(|row| (row.player, row.objects))
                            .collect();
                        accepted = self.sacrifice_after_life_choices(
                            saved,
                            self.window
                                .resolving()
                                .ok_or(EffectError::WrongResolutionFrame)?
                                .controller(),
                        )?;
                    }
                    let replacements =
                        self.begin_selection_replacements(&cursor, &operation, accepted.clone())?;
                    if (!replacements.sources().is_empty()
                        || matches!(
                            operation,
                            ResolutionSelectionOperation::RevealedHand { .. }
                                | ResolutionSelectionOperation::ReturnGraveyardCardsToHand { .. }
                                | ResolutionSelectionOperation::OwnedCommandersToHand
                                | ResolutionSelectionOperation::ExileGraveyardTargets { .. }
                                | ResolutionSelectionOperation::ExileChosenGraveyardCards { .. }
                                | ResolutionSelectionOperation::GraveyardCreatureToHand
                                | ResolutionSelectionOperation::ExileChosen { .. }
                                | ResolutionSelectionOperation::ExileFromHand { .. }
                                | ResolutionSelectionOperation::ExileLibrarySegment { .. }
                                | ResolutionSelectionOperation::ShuffleAnnouncedTargets { .. }
                                | ResolutionSelectionOperation::PutInLibrary { .. }
                                | ResolutionSelectionOperation::PutHandCardOnBottom
                                | ResolutionSelectionOperation::ExileAnnouncedTargets { .. }
                                | ResolutionSelectionOperation::MillEachPlayer { .. }
                                | ResolutionSelectionOperation::LibraryCohortToGraveyard { .. }
                                | ResolutionSelectionOperation::ExileLinkedTargets { .. }
                                | ResolutionSelectionOperation::ReturnAnnouncedTargets { .. }
                                | ResolutionSelectionOperation::ReturnSingle { .. }
                                | ResolutionSelectionOperation::ReturnToHand { .. }
                                | ResolutionSelectionOperation::OtherControlledCreature {
                                    sacrifice: false,
                                    ..
                                }
                                | ResolutionSelectionOperation::ExileSingle { .. }
                                | ResolutionSelectionOperation::WholeGroup {
                                    destination: PublicZoneDestination::Hand
                                        | PublicZoneDestination::Exile,
                                    ..
                                }
                        ))
                        && !replacements.candidates().is_empty()
                    {
                        ResolutionSelectionStage::Replacing {
                            selections: accepted,
                            batch: Box::new(replacements),
                        }
                    } else {
                        let groups = self.selection_owner_groups(&accepted)?;
                        ResolutionSelectionStage::Ordering {
                            replacements: None,
                            selections: accepted,
                            groups,
                            next_owner: 0,
                            orders: vec![],
                        }
                    }
                }
                ResolutionSelectionStage::Replacing {
                    selections,
                    batch: mut replacements,
                } => {
                    if let Some(frame) = replacements.progress(self.revision)? {
                        self.pending_work = Some(PendingWork::ResolutionSelection(Box::new(
                            ResolutionSelectionWork {
                                cursor,
                                operation,
                                stage: ResolutionSelectionStage::Replacing {
                                    selections,
                                    batch: replacements,
                                },
                            },
                        )));
                        return Ok(Some(frame));
                    }
                    let groups = self.selection_arrival_groups(&selections, Some(&replacements))?;
                    ResolutionSelectionStage::Ordering {
                        selections,
                        replacements: Some(replacements),
                        groups,
                        next_owner: 0,
                        orders: vec![],
                    }
                }
                ResolutionSelectionStage::Ordering {
                    replacements,
                    selections,
                    groups,
                    mut next_owner,
                    mut orders,
                } => {
                    if orders.len() != next_owner || next_owner > groups.len() {
                        return Err(EffectError::InconsistentSelectionWork);
                    }
                    next_graveyard_group(&groups, &mut orders)?;
                    next_owner = orders.len();
                    if next_owner < groups.len() {
                        let work = ResolutionSelectionWork {
                            cursor,
                            operation,
                            stage: ResolutionSelectionStage::Ordering {
                                replacements,
                                selections,
                                groups,
                                next_owner,
                                orders,
                            },
                        };
                        let frame = self
                            .work_selection_frame(&work)
                            .ok_or(EffectError::InconsistentSelectionWork)?;
                        self.pending_work = Some(PendingWork::ResolutionSelection(Box::new(work)));
                        return Ok(Some(frame));
                    }
                    ResolutionSelectionStage::Ready(self.selection_batch_replaced(
                        &selections,
                        orders,
                        replacements,
                    )?)
                }
                ResolutionSelectionStage::Ready(batch) => {
                    if matches!(
                        operation,
                        ResolutionSelectionOperation::Discard
                            | ResolutionSelectionOperation::RandomDiscard { .. }
                    ) {
                        cursor.results.discarded_types.clear();
                        cursor.results.last_nonland_discarded = 0;
                        cursor.discarded_nonland_mana_value = None;
                        if !batch.logical.is_empty() {
                            let current = self.evaluated_characteristics()?;
                            let nonland_count = batch
                                .logical
                                .iter()
                                .filter(|selection| {
                                    current
                                        .get(selection.object)
                                        .is_some_and(|card| !card.has_type(CardType::Land))
                                })
                                .count();
                            cursor.results.last_nonland_discarded =
                                u16::try_from(nonland_count).unwrap_or(u16::MAX);
                            cursor.results.discarded_types = batch
                                .logical
                                .iter()
                                .filter_map(|selection| current.get(selection.object))
                                .flat_map(|card| card.current_types.iter().copied())
                                .collect();
                        }
                        if batch.logical.len() == 1 {
                            let object = batch.logical[0].object;
                            let current = self.evaluated_characteristics()?;
                            if current
                                .get(object)
                                .is_some_and(|value| !value.has_type(CardType::Land))
                            {
                                cursor.discarded_nonland_mana_value =
                                    self.copiable_program(object).map(Program::mana_value);
                            }
                        }
                    }
                    if matches!(
                        operation,
                        ResolutionSelectionOperation::SacrificeContributor { .. }
                    ) {
                        let evaluated = self.evaluated_characteristics()?;
                        cursor.impulse.sacrificed_types = batch
                            .logical
                            .first()
                            .and_then(|row| evaluated.get(row.object))
                            .map(|card| card.current_creature_subtypes.clone());
                    }
                    let repeat_random_sacrifice = matches!(
                        operation,
                        ResolutionSelectionOperation::RandomSacrifice { .. }
                    ) && if let Some(row) = batch.logical.first() {
                        self.evaluated_characteristics()?
                            .get(row.object)
                            .is_some_and(|permanent| !permanent.has_type(CardType::Land))
                    } else {
                        false
                    };
                    if operation.is_sacrifice() {
                        cursor.results.last_sacrifice = !batch.logical.is_empty();
                        let evaluated = self.evaluated_characteristics()?;
                        cursor.results.last_sacrificed_creature = batch
                            .logical
                            .first()
                            .and_then(|row| evaluated.get(row.object))
                            .filter(|permanent| permanent.is_creature())
                            .and_then(|permanent| permanent.power_toughness.as_ref())
                            .map(|stats| stats.derived.clone());
                        cursor.results.last_sacrificed_mv = batch
                            .logical
                            .first()
                            .and_then(|row| self.permanent_mana_value(row.object));
                    }
                    if matches!(
                        operation,
                        ResolutionSelectionOperation::ExileChosenGraveyardCards { .. }
                    ) {
                        cursor.results.selected_graveyard_exile_creatures = 0;
                    }
                    let eval_lki = self.evaluated_characteristics()?;
                    cursor.results.last_action_lki = batch
                        .logical
                        .iter()
                        .filter_map(|row| eval_lki.get(row.object).cloned())
                        .collect();
                    cursor.results.last_action_succeeded = !batch.logical.is_empty();
                    cursor.results.last_action_count =
                        Some(RulesInteger::from_usize(batch.logical.len()));
                    self.commit_resolution_selection(&mut cursor, &operation, &batch, append)?;
                    cursor.revealed_piles = None;
                    cursor.results.sampled_graveyard_cards = None;
                    if matches!(
                        operation,
                        ResolutionSelectionOperation::RandomDiscard { .. }
                            | ResolutionSelectionOperation::RandomSacrifice { .. }
                    ) {
                        cursor.random_selection = None;
                    }
                    if repeat_random_sacrifice {
                        self.pending_work = Some(PendingWork::Resolution(cursor));
                        return Ok(None);
                    }
                    if matches!(
                        operation,
                        ResolutionSelectionOperation::WholeGroup {
                            destination: PublicZoneDestination::Graveyard,
                            ..
                        }
                    ) {
                        cursor.destroyed_permanents = Some(batch.logical.len());
                        cursor.results.last_moved_controllers =
                            batch.logical.iter().map(|row| row.player).collect();
                    }
                    if matches!(
                        operation,
                        ResolutionSelectionOperation::Discard
                            | ResolutionSelectionOperation::RandomDiscard { .. }
                    ) {
                        let mut discarded = BTreeMap::new();
                        for row in &batch.logical {
                            *discarded.entry(row.player).or_insert(0usize) += 1;
                        }
                        cursor.largest_discard = Some(discarded.into_values().max().unwrap_or(0));
                    }
                    #[cfg(test)]
                    let observed = (cursor.entry, cursor.instruction);
                    if matches!(
                        operation,
                        ResolutionSelectionOperation::OtherControlledCreature {
                            or_exile_source: true,
                            ..
                        }
                    ) && !batch.logical.is_empty()
                    {
                        cursor.instruction += 1;
                    }
                    self.pending_work = Some(PendingWork::Resolution(cursor));
                    self.advance_instruction()?;
                    #[cfg(test)]
                    super::characteristic_observations::instruction(
                        self,
                        "Completed",
                        observed.0,
                        observed.1,
                        None,
                    );
                    return Ok(None);
                }
            };
        }
    }
    /// Accept one exact executor frame on a tentative clone of this accepted state.
    ///
    /// # Errors
    /// Rejects stale/exhausted revisions, wrong frames/actors/reply kinds,
    /// incorrect cardinality, duplicate/outside members and incomplete owner
    /// permutations. Propagates later stage/commit failures without mutating self.
    pub(super) fn resume_resolution_selection(
        &self,
        expected: u64,
        frame: &EffectDecision,
        actor: SeatId,
        reply: DecisionReply,
    ) -> Result<RuleProgress, EffectError> {
        let revision = self.next_revision(expected)?;
        if self.selection_frame().as_ref() != Some(frame) {
            return Err(EffectError::WrongSelectionFrame);
        }
        if actor != frame.chooser() {
            return Err(EffectError::WrongChooser);
        }
        let mut next = self.clone();
        let Some(PendingWork::ResolutionSelection(work)) = next.pending_work.as_mut() else {
            return Err(EffectError::WrongSelectionFrame);
        };
        match (&mut work.stage, reply) {
            (
                ResolutionSelectionStage::Replacing {
                    batch: replacements,
                    ..
                },
                DecisionReply::ZoneReplacement(reply),
            ) => {
                let EffectDecision::ZoneReplacement(bound) = frame else {
                    return Err(ZoneReplacementError::WrongFrame.into());
                };
                replacements.accept(self.revision, bound, actor, reply)?;
            }
            (
                ResolutionSelectionStage::Choosing {
                    requirements,
                    next_player,
                    accepted,
                },
                DecisionReply::SelectResolution(objects),
            ) => {
                let requirement = requirements
                    .get(*next_player)
                    .ok_or(EffectError::InconsistentSelectionWork)?;
                if accepted.len() != *next_player {
                    return Err(EffectError::InconsistentSelectionWork);
                }
                if objects.len() > requirement.required {
                    return Err(EffectError::InvalidSelectionCount);
                }
                match &work.operation {
                    ResolutionSelectionOperation::CollectEvidence { minimum } => {
                        let chosen_mv = objects.iter().fold(RulesInteger::from(0), |acc, id| {
                            acc.plus(&self.permanent_mana_value(*id).unwrap_or_else(|| 0.into()))
                        });
                        if objects.is_empty() || chosen_mv < *minimum {
                            return Err(EffectError::InvalidSelectionCount);
                        }
                    }
                    ResolutionSelectionOperation::SelectZoneCards { min, .. } => {
                        if objects.len() < *min {
                            return Err(EffectError::InvalidSelectionCount);
                        }
                    }
                    ResolutionSelectionOperation::ChooseObjects { min, .. } => {
                        let eff_min = (*min).min(requirement.required);
                        if objects.len() < eff_min {
                            return Err(EffectError::InvalidSelectionCount);
                        }
                    }
                    ResolutionSelectionOperation::PayLifeToSaveCreatures { .. }
                    | ResolutionSelectionOperation::OtherControlledCreature {
                        or_exile_source: true,
                        ..
                    }
                    | ResolutionSelectionOperation::AttachToSource { optional: true, .. }
                    | ResolutionSelectionOperation::EnterAurasAttachedToSource { .. }
                    | ResolutionSelectionOperation::CopyDifferentlyNamedTokens
                    | ResolutionSelectionOperation::Untap { .. }
                    | ResolutionSelectionOperation::GraveyardCreatureToHand => {}
                    _ => {
                        if objects.len() != requirement.required {
                            return Err(EffectError::InvalidSelectionCount);
                        }
                    }
                }
                let unique: BTreeSet<_> = objects.iter().copied().collect();
                if unique.len() != objects.len() {
                    return Err(EffectError::DuplicateSelection);
                }
                if !objects
                    .iter()
                    .all(|object| requirement.domain.contains(object))
                {
                    return Err(EffectError::SelectionOutsideDomain);
                }
                if let ResolutionSelectionOperation::ChooseObjects {
                    distinct_powers,
                    max_total_power,
                    ..
                } = &work.operation
                    && (*distinct_powers || max_total_power.is_some())
                    && !objects.is_empty()
                {
                    let current = self.evaluated_characteristics()?;
                    let mut seen_powers = BTreeSet::new();
                    let mut total_power = RulesInteger::from(0);
                    for id in &objects {
                        let Some(power) = current
                            .get(*id)
                            .and_then(|view| view.power_toughness.as_ref())
                            .map(|pt| pt.derived.power.clone())
                        else {
                            return Err(EffectError::SelectionOutsideDomain);
                        };
                        if *distinct_powers && !seen_powers.insert(power.clone()) {
                            return Err(EffectError::SelectionOutsideDomain);
                        }
                        total_power = total_power.plus(&power);
                    }
                    if let Some(limit) = max_total_power
                        && total_power > *limit
                    {
                        return Err(EffectError::SelectionOutsideDomain);
                    }
                }
                if work.operation == ResolutionSelectionOperation::CopyDifferentlyNamedTokens {
                    let current = self.evaluated_characteristics()?;
                    let names = objects
                        .iter()
                        .map(|id| {
                            current
                                .get(*id)
                                .map(|view| &view.current_names)
                                .ok_or(EffectError::SelectionOutsideDomain)
                        })
                        .collect::<Result<Vec<_>, _>>()?;
                    if !self.name_rules.pairwise_different(&names) {
                        return Err(EffectError::SelectionOutsideDomain);
                    }
                }
                accepted.push(ResolutionAcceptedSelection {
                    player: requirement.player,
                    objects,
                });
                *next_player = next_player
                    .checked_add(1)
                    .ok_or(EffectError::InconsistentSelectionWork)?;
            }
            (
                ResolutionSelectionStage::Ordering {
                    groups,
                    next_owner,
                    orders,
                    ..
                },
                DecisionReply::OrderResolutionGraveyard(top_to_bottom),
            ) => {
                let group = groups
                    .get(*next_owner)
                    .ok_or(EffectError::InconsistentSelectionWork)?;
                if orders.len() != *next_owner {
                    return Err(EffectError::InconsistentSelectionWork);
                }
                validate_graveyard_order(group, &top_to_bottom)?;
                orders.push(ResolutionAcceptedGraveyardOrder {
                    owner: actor,
                    top_to_bottom,
                });
                *next_owner = next_owner
                    .checked_add(1)
                    .ok_or(EffectError::InconsistentSelectionWork)?;
            }
            (_, _) => return Err(EffectError::WrongDecisionKind),
        }
        next.revision = revision;
        next.progress_rules(
            DamageJournal::default(),
            vec![],
            vec![],
            DrawJournal::default(),
            RulesJournal::default(),
        )
    }
    pub(super) fn selection_owner_groups(
        &self,
        selections: &[ResolutionAcceptedSelection],
    ) -> Result<Vec<ResolutionGraveyardGroup>, EffectError> {
        self.zone_arrival_groups(
            &selections
                .iter()
                .flat_map(|s| s.objects.iter().copied())
                .collect::<Vec<_>>(),
            None,
        )
    }
    pub(super) fn selection_arrival_groups(
        &self,
        selections: &[ResolutionAcceptedSelection],
        replacements: Option<&ZoneReplacementBatch>,
    ) -> Result<Vec<ResolutionGraveyardGroup>, EffectError> {
        self.zone_arrival_groups(
            &selections
                .iter()
                .flat_map(|s| s.objects.iter().copied())
                .collect::<Vec<_>>(),
            replacements,
        )
    }
    pub(super) fn selection_batch_replaced(
        &self,
        selections: &[ResolutionAcceptedSelection],
        orders: Vec<ResolutionAcceptedGraveyardOrder>,
        replacements: Option<Box<ZoneReplacementBatch>>,
    ) -> Result<ResolutionSelectionBatch, EffectError> {
        let mut players = BTreeMap::new();
        let mut objects = vec![];
        for selection in selections {
            for &id in &selection.objects {
                if players.insert(id, selection.player).is_some() {
                    return Err(EffectError::InconsistentSelectionWork);
                }
                objects.push(id);
            }
        }
        let plan = self.zone_move_plan(&objects, &orders, replacements.as_deref())?;
        let rows = |ids: &[ObjectId]| {
            ids.iter()
                .map(|id| {
                    Ok(ResolutionSelectedObject {
                        object: *id,
                        player: *players
                            .get(id)
                            .ok_or(EffectError::InconsistentSelectionWork)?,
                    })
                })
                .collect::<Result<Vec<_>, EffectError>>()
        };
        Ok(ResolutionSelectionBatch {
            selections: selections.to_vec(),
            logical: rows(&plan.logical)?,
            application: rows(
                &plan
                    .application
                    .iter()
                    .map(|(id, _)| *id)
                    .collect::<Vec<_>>(),
            )?,
            owner_orders: orders,
            token_departures: plan.token_departures,
            replacements,
        })
    }
    fn creatures_controllable_for_life_choice(
        &self,
        evaluated: &EvaluatedCharacteristics,
        player: SeatId,
    ) -> Vec<ObjectId> {
        self.window
            .cards()
            .iter()
            .filter(|card| {
                card.zone() == ObjectZone::Battlefield
                    && evaluated
                        .get(card.id())
                        .is_some_and(|view| view.is_creature() && view.controller == Some(player))
            })
            .map(GameObject::id)
            .collect()
    }

    fn sacrifice_after_life_choices(
        &self,
        saved: &BTreeMap<SeatId, Vec<ObjectId>>,
        controller: SeatId,
    ) -> Result<Vec<ResolutionAcceptedSelection>, EffectError> {
        let evaluated = self.evaluated_characteristics()?;
        self.window
            .apnap_players()?
            .map(|player| {
                let keep = saved
                    .get(&player)
                    .ok_or(EffectError::InconsistentSelectionWork)?;
                let domain = self.creatures_controllable_for_life_choice(&evaluated, player);
                if !keep.iter().all(|id| domain.contains(id)) {
                    return Err(EffectError::InconsistentSelectionWork);
                }
                let protected =
                    self.opposing_sacrifice_prohibited(&evaluated, player, controller)?;
                let objects = domain
                    .into_iter()
                    .filter(|id| {
                        !protected && !keep.contains(id) && !evaluated.sacrifice_prohibited(*id)
                    })
                    .collect();
                Ok(ResolutionAcceptedSelection { player, objects })
            })
            .collect()
    }

    fn commit_resolution_selection(
        &mut self,
        cursor: &mut ResolutionCursor,
        operation: &ResolutionSelectionOperation,
        batch: &ResolutionSelectionBatch,
        append: &mut RulesJournal,
    ) -> Result<(), EffectError> {
        let entry = self
            .window
            .resolving()
            .ok_or(EffectError::WrongResolutionFrame)?;
        if entry.id() != cursor.entry {
            return Err(EffectError::WrongResolutionFrame);
        }
        let actual = self
            .payloads
            .get(&cursor.entry)
            .and_then(StackPayload::instructions)
            .and_then(|body| body.get(cursor.instruction))
            .ok_or(EffectError::InconsistentSelectionWork)?;
        let exile_siege = matches!(actual, Instruction::ExileDefeatedSiege);
        let exile_source = false;
        let delay_exile_return = false;
        let delay_cohort_return = false;
        let imprint = false;
        let inherit_from_exile = false;
        let play_exiled_card: Option<SeatId> = None;
        let independent_link: Option<(ObjectId, crate::program::LinkedExileGroup)> = None;
        let players = match (operation, actual) {
            (
                ResolutionSelectionOperation::RevealedHand {
                    chooser,
                    minimum_mana_value,
                    nonland_only,
                    filter,
                    count,
                    action,
                },
                Instruction::ChooseRevealedHandCard {
                    minimum_mana_value: minimum,
                    nonland_only: nonland,
                    filter: expected_filter,
                    count: required,
                    action: expected,
                },
            ) if *chooser == entry.controller()
                && minimum_mana_value == minimum
                && nonland_only == nonland
                && filter == expected_filter
                && count == required
                && action == expected =>
            {
                AffectedPlayers::One(ResolvingPlayer::TargetPlayer)
            }
            (
                ResolutionSelectionOperation::ExileLinkedTargets { objects, source },
                Instruction::ExileOnePerPlayerUntilSourceLeaves,
            ) if *objects == self.legal_permanent_targets()?
                && self
                    .execution_bindings(entry.id())
                    .and_then(|b| b.trigger.as_ref())
                    .is_some_and(|t| t.source.object.id() == *source) =>
            {
                AffectedPlayers::EachPlayer
            }
            (
                ResolutionSelectionOperation::ExileAnnouncedTargets { objects },
                Instruction::ExileDelayedObjects(ids),
            ) if *objects
                == ids
                    .iter()
                    .copied()
                    .filter(|id| {
                        self.window
                            .cards()
                            .get(*id)
                            .is_some_and(|p| p.zone() == ObjectZone::Battlefield)
                    })
                    .collect() =>
            {
                AffectedPlayers::EachPlayer
            }
            (
                ResolutionSelectionOperation::ExileAnnouncedTargets { objects },
                Instruction::AffectPermanentTargets {
                    action: crate::program::PermanentTargetAction::Exile,
                    ..
                },
            )
            | (
                ResolutionSelectionOperation::DestroyAnnouncedTargets { objects },
                Instruction::AffectPermanentTargets {
                    action: crate::program::PermanentTargetAction::Destroy,
                    ..
                },
            )
            | (
                ResolutionSelectionOperation::ReturnAnnouncedTargets { objects },
                Instruction::AffectPermanentTargets {
                    action: crate::program::PermanentTargetAction::ReturnToHand,
                    ..
                },
            ) if *objects == self.legal_permanent_targets()? => AffectedPlayers::EachPlayer,
            (
                ResolutionSelectionOperation::ReturnSingle { object },
                Instruction::ReturnDelayedPermanentToHand(expected),
            ) if object == expected => AffectedPlayers::EachPlayer,
            (
                ResolutionSelectionOperation::Sacrifice { filter },
                Instruction::SacrificeDelayedObjects(objects),
            ) if filter.any_types.is_empty()
                && filter.required_types.is_empty()
                && filter.forbidden_types.is_empty()
                && batch
                    .logical
                    .iter()
                    .all(|selection| objects.contains(&selection.object)) =>
            {
                AffectedPlayers::One(crate::program::ResolvingPlayer::Controller)
            }
            (
                ResolutionSelectionOperation::Sacrifice { filter },
                Instruction::EndStepCast {
                    kind: crate::program::EndStepCast::Blitz,
                    ..
                }
                | Instruction::SacrificeEvoked
                | Instruction::SacrificeStateTriggerSource
                | Instruction::ResolveCounterKeyword(
                    crate::program::CounterKeyword::Fading(_)
                    | crate::program::CounterKeyword::VanishingLastCounter,
                ),
            ) if filter.required_types.is_empty()
                && filter.forbidden_types.is_empty()
                && matches!(entry.kind(),StackKind::TriggeredAbility{source} | StackKind::ActivatedAbility{source} if batch.logical.iter().all(|selection|selection.object==if let Instruction::EndStepCast{permanent,..}=actual{*permanent}else{source.id()})) =>
            {
                AffectedPlayers::One(crate::program::ResolvingPlayer::Controller)
            }
            (
                ResolutionSelectionOperation::RandomSacrifice { object },
                Instruction::SacrificeRandomUntilLand,
            ) if cursor.random_selection == Some(*object) => {
                AffectedPlayers::One(ResolvingPlayer::TargetPlayer)
            }
            (ResolutionSelectionOperation::Discard, Instruction::Discard { players, .. }) => {
                *players
            }
            (
                ResolutionSelectionOperation::ExileSingle {
                    object,
                    source: None,
                    monarch_controller: None,
                },
                Instruction::ExileDefeatedSiege,
            ) if matches!(entry.kind(), StackKind::TriggeredAbility {source} | StackKind::ActivatedAbility {source} if *object==source.id()) => {
                AffectedPlayers::EachPlayer
            }
            (
                ResolutionSelectionOperation::Sacrifice { filter },
                Instruction::Sacrifice {
                    players,
                    filter: actual,
                    ..
                },
            ) if filter == actual => *players,
            _ => return Err(EffectError::InconsistentSelectionWork),
        };
        let players: BTreeSet<_> = match players {
            AffectedPlayers::One(_) if matches!(actual, Instruction::SacrificeEvoked) => {
                [self.source_sacrifice_player(entry, actual)].into()
            }
            players => self
                .selection_players(players, entry.controller(), Some(cursor))?
                .into_iter()
                .collect(),
        };
        let evaluated = self.evaluated_characteristics()?;
        let excluded = operation.excluded_types(cursor)?;
        let ability_source = match entry.kind() {
            StackKind::ActivatedAbility { source } | StackKind::TriggeredAbility { source } => {
                Some(source.id())
            }
            StackKind::CardSpell(id) => Some(*id),
            _ => None,
        };
        let mut unique = BTreeSet::new();
        for selected in &batch.logical {
            let object = self
                .window
                .cards()
                .get(selected.object)
                .ok_or(EffectError::InconsistentSelectionWork)?;
            if !unique.insert(selected.object)
                || !players.contains(&selected.player)
                || (operation.is_sacrifice()
                    && self.opposing_sacrifice_prohibited(
                        &evaluated,
                        selected.player,
                        entry.controller(),
                    )?)
                || !self.selection_eligible(
                    operation,
                    selected.player,
                    object,
                    ability_source,
                    &evaluated,
                    &excluded,
                )
            {
                return Err(EffectError::InconsistentSelectionWork);
            }
        }
        let application: BTreeMap<_, _> = batch
            .application
            .iter()
            .map(|row| (row.object, row.player))
            .collect();
        let logical: BTreeMap<_, _> = batch
            .logical
            .iter()
            .map(|row| (row.object, row.player))
            .collect();
        if application != logical || application.len() != batch.application.len() {
            return Err(EffectError::InconsistentSelectionWork);
        }
        let mut selections = BTreeMap::<SeatId, Vec<ObjectId>>::new();
        for row in &batch.logical {
            selections.entry(row.player).or_default().push(row.object);
        }
        let selections: Vec<_> = selections
            .into_iter()
            .map(|(player, objects)| ResolutionAcceptedSelection { player, objects })
            .collect();
        if matches!(
            operation,
            ResolutionSelectionOperation::RevealedHand { .. }
                | ResolutionSelectionOperation::ReturnGraveyardCardsToHand { .. }
                | ResolutionSelectionOperation::OwnedCommandersToHand
                | ResolutionSelectionOperation::ExileGraveyardTargets { .. }
                | ResolutionSelectionOperation::ExileChosenGraveyardCards { .. }
                | ResolutionSelectionOperation::GraveyardCreatureToHand
                | ResolutionSelectionOperation::ExileChosen { .. }
                | ResolutionSelectionOperation::ExileFromHand { .. }
                | ResolutionSelectionOperation::ExileLibrarySegment { .. }
                | ResolutionSelectionOperation::ShuffleAnnouncedTargets { .. }
                | ResolutionSelectionOperation::PutInLibrary { .. }
                | ResolutionSelectionOperation::PutHandCardOnBottom
                | ResolutionSelectionOperation::ExileAnnouncedTargets { .. }
                | ResolutionSelectionOperation::MillEachPlayer { .. }
                | ResolutionSelectionOperation::LibraryCohortToGraveyard { .. }
                | ResolutionSelectionOperation::ExileLinkedTargets { .. }
                | ResolutionSelectionOperation::ReturnAnnouncedTargets { .. }
                | ResolutionSelectionOperation::ReturnSingle { .. }
                | ResolutionSelectionOperation::ReturnToHand { .. }
                | ResolutionSelectionOperation::OtherControlledCreature {
                    sacrifice: false,
                    ..
                }
                | ResolutionSelectionOperation::WholeGroup {
                    destination: PublicZoneDestination::Hand | PublicZoneDestination::Exile,
                    ..
                }
        ) && !batch.logical.is_empty()
            && batch.replacements.is_none()
        {
            return Err(EffectError::InconsistentSelectionWork);
        }
        if let Some(replacements) = &batch.replacements {
            replacements.verify(self, cursor, operation, batch.selections.clone())?;
        }
        let expected_groups =
            self.selection_arrival_groups(&selections, batch.replacements.as_deref())?;
        if expected_groups.len() != batch.owner_orders.len() {
            return Err(EffectError::InconsistentSelectionWork);
        }
        for (group, order) in expected_groups.iter().zip(&batch.owner_orders) {
            if group.owner != order.owner
                || group.entrants.len() != order.top_to_bottom.len()
                || group.entrants.iter().copied().collect::<BTreeSet<_>>()
                    != order.top_to_bottom.iter().copied().collect::<BTreeSet<_>>()
            {
                return Err(EffectError::InconsistentSelectionWork);
            }
        }
        let rebuilt = self.selection_batch_replaced(
            &batch.selections,
            batch.owner_orders.clone(),
            batch.replacements.clone(),
        )?;
        if rebuilt != *batch
            || matches!(
                operation,
                ResolutionSelectionOperation::Discard
                    | ResolutionSelectionOperation::RandomDiscard { .. }
            ) && !batch.token_departures.is_empty()
        {
            return Err(EffectError::InconsistentSelectionWork);
        }
        let mut payment_adjustments = BTreeMap::new();
        if let ResolutionSelectionOperation::PayLifeToSaveCreatures { cost, saved } = operation {
            if batch.selections != self.sacrifice_after_life_choices(saved, entry.controller())? {
                return Err(EffectError::InconsistentSelectionWork);
            }
            for (&player, objects) in saved {
                let amount = cost.times(&RulesInteger::from_usize(objects.len()));
                if amount.is_positive() {
                    if self.life.get(player).is_none_or(|total| total < &amount) {
                        return Err(EffectError::InconsistentSelectionWork);
                    }
                    payment_adjustments.insert(player, crate::life::LifeAdjustment::Lose(amount));
                }
            }
        }
        if payment_adjustments.is_empty()
            && batch.logical.is_empty()
            && !batch
                .replacements
                .as_ref()
                .is_some_and(|b| b.has_destruction_saves())
        {
            return Ok(());
        }
        let before = self.rules_snapshot(None)?;
        let mut regenerated = self.apply_regenerations(batch.replacements.as_deref())?;
        regenerated.extend(
            self.life
                .apply_adjustments(&payment_adjustments)
                .map_err(|_| EffectError::InconsistentSelectionWork)?
                .into_iter()
                .map(RulesOccurrence::LifeChanged),
        );
        if batch.logical.is_empty() {
            return self.emit_from(
                EventCause::Instruction {
                    entry: cursor.entry,
                    instruction: cursor.instruction,
                },
                &before,
                None,
                append,
                |_, _, _| Ok(regenerated),
            );
        }
        let (window, movements) = self.window.record_resolution_selection_moves(
            self.window.revision(),
            cursor.entry,
            batch
                .application
                .iter()
                .map(|row| {
                    operation.movement(
                        row,
                        batch.replacements.as_ref().map_or(
                            Ok(crate::program::PublicZoneDestination::Graveyard),
                            |work| work.destination(row.object),
                        )?,
                        self.window
                            .cards()
                            .get(row.object)
                            .ok_or(EffectError::InconsistentSelectionWork)?
                            .zone(),
                    )
                })
                .collect::<Result<Vec<_>, EffectError>>()?,
        )?;
        let mut movements: BTreeMap<_, _> = movements
            .into_iter()
            .map(|movement| (movement.before.id(), movement))
            .collect();
        let mut ordered = Vec::with_capacity(batch.logical.len());
        for row in &batch.logical {
            ordered.push((
                movements
                    .remove(&row.object)
                    .ok_or(EffectError::InconsistentSelectionWork)?,
                operation.reason(row.player)?,
            ));
        }
        if !movements.is_empty() {
            return Err(EffectError::InconsistentSelectionWork);
        }
        self.window = window;
        let newly_entered_battlefield: Vec<ObjectId> = ordered
            .iter()
            .filter(|(movement, _)| {
                movement.after.zone() == ObjectZone::Battlefield
                    && movement.before.zone() != ObjectZone::Battlefield
            })
            .map(|(movement, _)| movement.after.id())
            .collect();
        if !newly_entered_battlefield.is_empty() {
            self.characteristics
                .register_battlefield_entries(&newly_entered_battlefield)?;
        }
        cursor.results.last_moved_objects = ordered
            .iter()
            .flat_map(|(movement, _)| movement.arrivals())
            .collect();
        if matches!(
            operation,
            ResolutionSelectionOperation::DestroySingle { .. }
        ) {
            cursor.results.destroyed_target_controller =
                ordered.first().and_then(|(movement, _)| {
                    if matches!(movement.after.zone(), ObjectZone::Graveyard(_)) {
                        movement.before.controller()
                    } else {
                        None
                    }
                });
        }
        if exile_siege {
            cursor.impulse.exiled = ordered.first().and_then(|(movement, _)| {
                (movement.after.zone() == ObjectZone::Exile && movement.after.card_id().is_some())
                    .then_some(movement.after.id())
            });
        }
        if matches!(
            operation,
            ResolutionSelectionOperation::DestroyAnnouncedTargets { .. }
        ) && self
            .payloads
            .get(&cursor.entry)
            .and_then(StackPayload::instructions)
            .and_then(|ops| ops.get(cursor.instruction + 1))
            == Some(&Instruction::ReturnCardsDestroyedThisWay)
        {
            cursor.results.destroyed_cards_to_graveyard = ordered
                .iter()
                .flat_map(|(movement, _)| movement.arrivals())
                .filter(|id| {
                    self.window.cards().get(*id).is_some_and(|card| {
                        card.card_id().is_some() && matches!(card.zone(), ObjectZone::Graveyard(_))
                    })
                })
                .collect();
        }
        if (matches!(
            operation,
            ResolutionSelectionOperation::ExileAnnouncedTargets { .. }
        ) || exile_source)
            && matches!(
                self.payloads
                    .get(&cursor.entry)
                    .and_then(StackPayload::instructions)
                    .and_then(|ops| ops.get(cursor.instruction + 1)),
                Some(Instruction::ReturnExiledTargetCohort { .. })
            )
        {
            cursor.impulse.flickered = ordered
                .iter()
                .filter(|(movement, _)| movement.after.zone() == ObjectZone::Exile)
                .flat_map(|(movement, _)| movement.arrivals())
                .filter(|id| {
                    self.window
                        .cards()
                        .get(*id)
                        .is_some_and(|object| object.card_id().is_some())
                })
                .collect();
        }
        if delay_cohort_return {
            let exiled = ordered
                .iter()
                .flat_map(|(movement, _)| movement.arrivals())
                .filter(|id| {
                    self.window.cards().get(*id).is_some_and(|card| {
                        card.zone() == ObjectZone::Exile && card.card_id().is_some()
                    })
                })
                .collect();
            self.delay_spell_return(cursor.entry, exiled)?;
        }
        if let ResolutionSelectionOperation::ExileLinkedTargets { source, .. } = operation {
            for (movement, _) in &ordered {
                if movement.after.zone() == ObjectZone::Exile {
                    self.linked_exiles
                        .entry(*source)
                        .or_default()
                        .extend(movement.arrivals());
                }
            }
        }
        if let Some(player) = play_exiled_card {
            for (movement, _) in &ordered {
                if movement.after.zone() == ObjectZone::Exile && movement.after.card_id().is_some()
                {
                    self.card_play_permissions.insert(CardPlayPermission {
                        zone: CardPlayZone::Exile,
                        object: movement.after.id(),
                        player,
                        duration: CardPlayDuration::WhileInZone,
                        any_color: false,
                        free: false,
                        cast_only: false,
                    });
                }
            }
        }
        if inherit_from_exile {
            for (movement, _) in &ordered {
                self.remember_exiled_ability_donor(cursor.entry, movement);
            }
        }
        if let ResolutionSelectionOperation::ExileSingle {
            source,
            monarch_controller,
            ..
        } = operation
        {
            for (movement, _) in &ordered {
                if movement.after.zone() == ObjectZone::Exile {
                    if let Some(link) = independent_link {
                        self.independent_exile_links
                            .entry(link)
                            .or_default()
                            .extend(movement.arrivals());
                    }
                    if imprint {
                        self.remember_imprinted_card(cursor.entry, movement);
                    }
                    if let Some(source) = source {
                        self.linked_exiles
                            .entry(*source)
                            .or_default()
                            .extend(movement.arrivals());
                    }
                    if delay_exile_return {
                        self.schedule_return_at_next_end_step(
                            cursor.entry,
                            movement.arrivals().collect(),
                        )?;
                    }
                    if let Some(controller) = monarch_controller {
                        self.monarch_linked_exiles
                            .extend(movement.arrivals().map(|id| (id, *controller)));
                    }
                }
            }
        }
        if matches!(
            operation,
            ResolutionSelectionOperation::LibraryCohortToGraveyard { .. }
                | ResolutionSelectionOperation::ExileLibrarySegment { .. }
        ) {
            let visible = self.evaluated_characteristics()?;
            cursor.results.moved_library_types = ordered
                .iter()
                .filter(|(movement, _)| {
                    matches!(
                        movement.after.zone(),
                        ObjectZone::Graveyard(_)
                            | ObjectZone::Exile
                            | ObjectZone::Battlefield
                            | ObjectZone::Command
                    )
                })
                .filter_map(|(movement, _)| visible.get(movement.after.id()))
                .flat_map(|view| view.current_types.iter().copied())
                .collect();
            if matches!(
                operation,
                ResolutionSelectionOperation::ExileLibrarySegment { .. }
            ) {
                cursor.results.revealed_top_types = cursor.results.moved_library_types.clone();
                cursor.results.revealed_top_card = ordered.first().map(|(m, _)| m.after.id());
                cursor.results.last_moved_objects =
                    ordered.iter().map(|(m, _)| m.after.id()).collect();
            }
        }
        if let ResolutionSelectionOperation::MillEachPlayer { amount, segments } = operation {
            cursor.impulse.milled = ordered
                .iter()
                .filter(|(movement, _)| matches!(movement.after.zone(), ObjectZone::Graveyard(_)))
                .map(|(movement, _)| movement.after.id())
                .collect();
            for player in self.window.apnap_players()? {
                let segment = segments
                    .get(&player)
                    .ok_or(EffectError::InconsistentSelectionWork)?;
                let public_zone_moves = segment
                    .iter()
                    .map(|id| {
                        let (movement, _) = ordered
                            .iter()
                            .find(|(movement, _)| movement.before.id() == *id)
                            .ok_or(EffectError::InconsistentSelectionWork)?;
                        let destination = match movement.after.zone() {
                            ObjectZone::Graveyard(_) => PublicZoneDestination::Graveyard,
                            ObjectZone::Exile => PublicZoneDestination::Exile,
                            ObjectZone::Hand(_) => PublicZoneDestination::Hand,
                            _ => return Err(EffectError::InconsistentSelectionWork),
                        };
                        Ok(LibraryPublicMove {
                            before: *id,
                            after: movement.after.id(),
                            destination,
                        })
                    })
                    .collect::<Result<Vec<_>, EffectError>>()?;
                let graveyard_before_top_first = batch
                    .owner_orders
                    .iter()
                    .find(|o| o.owner == player)
                    .map(|o| o.top_to_bottom.clone())
                    .unwrap_or_default();
                regenerated.push(RulesOccurrence::LibraryAction(LibraryAction {
                    kind: LibraryActionKind::Mill,
                    player,
                    requested: i64::from(*amount).into(),
                    looked_top_first: segment.clone(),
                    selected_before_top_first: segment.clone(),
                    public_zone_moves,
                    graveyard_before_top_first,
                    top_first: Vec::new(),
                    bottom_top_first: Vec::new(),
                }));
            }
        }
        if matches!(
            operation,
            ResolutionSelectionOperation::ExileChosenGraveyardCards { .. }
        ) {
            cursor.results.selected_graveyard_exile_creatures = ordered
                .iter()
                .filter(|(movement, _)| {
                    movement.after.zone() == ObjectZone::Exile
                        && evaluated
                            .get(movement.before.id())
                            .is_some_and(|card| card.is_creature())
                })
                .count();
        }
        let exiled_creatures = if matches!(
            operation,
            ResolutionSelectionOperation::ExileGraveyardTargets { .. }
        ) {
            for (movement, _) in &ordered {
                if movement.after.zone() == ObjectZone::Exile
                    && let Some((definition, _)) = before
                        .view()
                        .characteristics
                        .copiable_definition(&movement.before, &self.programs)
                {
                    self.departed_copy_sources
                        .insert(movement.before.id(), definition.clone());
                }
            }
            ordered
                .iter()
                .filter(|(movement, _)| {
                    movement.after.zone() == ObjectZone::Exile
                        && evaluated
                            .get(movement.before.id())
                            .is_some_and(|value| value.is_creature())
                })
                .count()
        } else {
            0
        };
        self.emit_from(
            EventCause::Instruction {
                entry: cursor.entry,
                instruction: cursor.instruction,
            },
            &before,
            None,
            append,
            |previous, current, controllers| {
                let moves = ordered
                    .iter()
                    .map(|(movement, reason)| {
                        zone_occurrence_at(previous, current, movement, *reason, controllers).map(
                            |value| {
                                occurrence_flow::madness_replaced(
                                    value,
                                    batch
                                        .replacements
                                        .as_ref()
                                        .is_some_and(|b| b.used_madness(movement.before.id())),
                                )
                            },
                        )
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                regenerated.extend(moves);
                Ok(regenerated)
            },
        )?;
        let committed_movements: Vec<_> = ordered.iter().map(|(m, _)| m.clone()).collect();
        self.apply_zone_replacement_extras_from_movements(
            batch.replacements.as_deref(),
            &committed_movements,
            EventCause::Instruction {
                entry: cursor.entry,
                instruction: cursor.instruction,
            },
            append,
        )?;
        if exiled_creatures > 0 {
            let player = self
                .window
                .resolving()
                .ok_or(EffectError::WrongResolutionFrame)?
                .controller();
            self.adjust_life_at_cut(
                cursor.entry,
                cursor.instruction,
                player,
                &crate::life::LifeAdjustment::Gain(RulesInteger::from_usize(exiled_creatures)),
                append,
            )?;
        }
        Ok(())
    }
    fn selection_eligible(
        &self,
        operation: &ResolutionSelectionOperation,
        player: SeatId,
        object: &GameObject,
        ability_source: Option<ObjectId>,
        evaluated: &EvaluatedCharacteristics,
        excluded: &BTreeSet<CreatureTypeId>,
    ) -> bool {
        let mana_value = || {
            if matches!(operation, ResolutionSelectionOperation::RevealedHand { .. }) {
                self.copiable_program(object.id()).map(Program::mana_value)
            } else {
                self.permanent_mana_value(object.id())
            }
        };
        if operation.is_sacrifice() && evaluated.sacrifice_prohibited(object.id()) {
            return false;
        }
        match operation {
            ResolutionSelectionOperation::ExileFromHand { allow_battlefield } => {
                (object.zone() == ObjectZone::Hand(player) && object.card_id().is_some())
                    || (*allow_battlefield
                        && object.zone() == ObjectZone::Battlefield
                        && evaluated
                            .get(object.id())
                            .is_some_and(|view| view.controller == Some(player)))
            }
            ResolutionSelectionOperation::PutHandCardOnBottom => {
                object.card_id().is_some() && object.zone() == ObjectZone::Hand(player)
            }
            ResolutionSelectionOperation::PutInLibrary { objects, .. } => {
                objects.contains(&object.id())
                    && ((object.zone() == ObjectZone::Battlefield
                        && object.controller() == Some(player))
                        || (object.card_id().is_some()
                            && object.zone() == ObjectZone::Graveyard(player))
                        || (matches!(object.zone(), ObjectZone::Exile | ObjectZone::Stack)
                            && object.owner() == player))
            }
            ResolutionSelectionOperation::ExileChosenGraveyardCards {
                player: owner,
                domain,
            } => {
                *owner == player
                    && domain.contains(&object.id())
                    && object.zone() == ObjectZone::Graveyard(player)
                    && object.card_id().is_some()
            }
            ResolutionSelectionOperation::ExileGraveyardTargets { objects } => {
                objects.contains(&object.id())
                    && object.card_id().is_some()
                    && object.zone() == ObjectZone::Graveyard(player)
            }
            ResolutionSelectionOperation::OwnedCommandersToHand => {
                object.owner() == player
                    && (object.zone() == ObjectZone::Command
                        || object.zone() == ObjectZone::Graveyard(player))
                    && evaluated
                        .get(object.id())
                        .is_some_and(|current| current.commander_owner == Some(player))
            }
            ResolutionSelectionOperation::ReturnGraveyardCardsToHand { objects } => {
                objects.contains(&object.id()) && object.zone() == ObjectZone::Graveyard(player)
            }
            ResolutionSelectionOperation::RevealedHand {
                minimum_mana_value,
                nonland_only,
                filter,
                ..
            } => {
                if object.zone() != ObjectZone::Hand(player) || object.card_id().is_none() {
                    false
                } else {
                    let mv = mana_value();
                    (!nonland_only
                        || evaluated
                            .get(object.id())
                            .is_some_and(|card| !card.has_type(CardType::Land)))
                        && filter.as_deref().is_none_or(|f| {
                            evaluated.get(object.id()).is_some_and(|card| {
                                f.matches_static_characteristics(card, &BTreeSet::new(), || {
                                    mv.clone()
                                })
                            })
                        })
                        && mv.is_some_and(|value| {
                            value >= RulesInteger::from_usize(usize::from(*minimum_mana_value))
                        })
                }
            }
            ResolutionSelectionOperation::GraveyardCreatureToHand => {
                object.card_id().is_some()
                    && object.zone() == ObjectZone::Graveyard(player)
                    && evaluated.get(object.id()).is_some_and(|p| p.is_creature())
            }
            ResolutionSelectionOperation::LibraryCohortToGraveyard {
                player: owner,
                cards,
            } => {
                *owner == player
                    && object.zone() == ObjectZone::Library(player)
                    && cards.contains(&object.id())
            }
            ResolutionSelectionOperation::MillEachPlayer { segments, .. } => {
                object.zone() == ObjectZone::Library(player)
                    && segments
                        .get(&player)
                        .is_some_and(|cards| cards.contains(&object.id()))
            }
            ResolutionSelectionOperation::DestroyAnnouncedTargets { objects } => {
                objects.contains(&object.id())
                    && object.zone() == ObjectZone::Battlefield
                    && evaluated.get(object.id()).is_some_and(|p| {
                        p.controller == Some(player)
                            && !p.has_keyword(KeywordAbility::Indestructible)
                    })
            }
            ResolutionSelectionOperation::ExileLibrarySegment {
                player: seat,
                cards,
            } => {
                *seat == player
                    && object.zone() == ObjectZone::Library(player)
                    && cards.contains(&object.id())
            }
            ResolutionSelectionOperation::ShuffleAnnouncedTargets { objects }
            | ResolutionSelectionOperation::ExileAnnouncedTargets { objects }
            | ResolutionSelectionOperation::ExileLinkedTargets { objects, .. }
            | ResolutionSelectionOperation::ReturnAnnouncedTargets { objects } => {
                objects.contains(&object.id())
                    && object.zone() == ObjectZone::Battlefield
                    && evaluated
                        .get(object.id())
                        .is_some_and(|p| p.controller == Some(player))
            }
            ResolutionSelectionOperation::ReturnSingle { object: bound } => {
                object.id() == *bound
                    && object.zone() == ObjectZone::Battlefield
                    && evaluated
                        .get(*bound)
                        .is_some_and(|p| p.controller == Some(player))
            }
            ResolutionSelectionOperation::DestroySingle { object: bound } => {
                object.id() == *bound
                    && object.zone() == ObjectZone::Battlefield
                    && evaluated.get(*bound).is_some_and(|p| {
                        p.controller == Some(player)
                            && !p.has_keyword(KeywordAbility::Indestructible)
                    })
            }
            ResolutionSelectionOperation::ExileSingle { object: bound, .. } => {
                object.id() == *bound
                    && object.zone() == ObjectZone::Battlefield
                    && evaluated
                        .get(*bound)
                        .is_some_and(|p| p.controller == Some(player))
            }
            ResolutionSelectionOperation::WholeGroup { group, destination } => {
                object.zone() == ObjectZone::Battlefield
                    && object.controller() == Some(player)
                    && evaluated.get(object.id()).is_some_and(|permanent| {
                        (*destination != PublicZoneDestination::Graveyard
                            || !permanent.has_keyword(KeywordAbility::Indestructible))
                            && group.matches_static_characteristics(permanent, excluded, mana_value)
                    })
            }
            ResolutionSelectionOperation::AttachToSource { eligible, .. }
            | ResolutionSelectionOperation::EnterAurasAttachedToSource { eligible, .. } => {
                eligible.contains(&object.id())
            }
            ResolutionSelectionOperation::CopyDifferentlyNamedTokens => {
                object.zone() == ObjectZone::Battlefield
                    && object.is_token()
                    && evaluated.get(object.id()).is_some_and(|view| {
                        view.controller == Some(player)
                            && !view.current_names.is_empty()
                            && (view.has_type(CardType::Artifact) || view.is_creature())
                    })
            }
            ResolutionSelectionOperation::Populate => {
                object.zone() == ObjectZone::Battlefield
                    && object.is_token()
                    && evaluated
                        .get(object.id())
                        .is_some_and(|view| view.controller == Some(player) && view.is_creature())
            }
            ResolutionSelectionOperation::Bolster { eligible, .. }
            | ResolutionSelectionOperation::Amass { eligible, .. } => {
                eligible.contains(&object.id())
            }
            ResolutionSelectionOperation::Untap { kind } => {
                object.zone() == ObjectZone::Battlefield
                    && evaluated
                        .get(object.id())
                        .is_some_and(|p| p.has_type(*kind))
            }
            ResolutionSelectionOperation::SacrificeContributor { eligible } => {
                eligible.contains(&object.id())
                    && object.zone() == ObjectZone::Battlefield
                    && evaluated.get(object.id()).and_then(|card| card.controller) == Some(player)
            }
            ResolutionSelectionOperation::PayLifeToSaveCreatures { .. }
            | ResolutionSelectionOperation::ChooseVow
            | ResolutionSelectionOperation::ChooseCreaturesToKeep
            | ResolutionSelectionOperation::SacrificeExceptVowed { .. } => {
                object.zone() == ObjectZone::Battlefield
                    && evaluated
                        .get(object.id())
                        .is_some_and(|card| card.controller == Some(player) && card.is_creature())
                    && !matches!(operation, ResolutionSelectionOperation::SacrificeExceptVowed {kept} if kept.contains(&object.id()))
            }
            ResolutionSelectionOperation::Blight { .. } => {
                object.zone() == ObjectZone::Battlefield
                    && evaluated
                        .get(object.id())
                        .is_some_and(|card| card.controller == Some(player) && card.is_creature())
            }
            ResolutionSelectionOperation::CollectEvidence { .. } => {
                object.card_id().is_some() && object.zone() == ObjectZone::Graveyard(player)
            }
            ResolutionSelectionOperation::SelectZoneCards {
                source_zone,
                filter,
                last_moved,
                ..
            }
            | ResolutionSelectionOperation::ChooseObjects {
                source_zone,
                filter,
                last_moved,
                ..
            } => {
                let zone_ok = match source_zone {
                    crate::program::SourceZoneFilter::Hand => {
                        object.card_id().is_some() && object.zone() == ObjectZone::Hand(player)
                    }
                    crate::program::SourceZoneFilter::Graveyard => {
                        object.card_id().is_some() && object.zone() == ObjectZone::Graveyard(player)
                    }
                    crate::program::SourceZoneFilter::HandOrGraveyard => {
                        object.card_id().is_some()
                            && matches!(
                                object.zone(),
                                ObjectZone::Hand(owner) | ObjectZone::Graveyard(owner) if owner == player
                            )
                    }
                    crate::program::SourceZoneFilter::Battlefield => {
                        object.zone() == ObjectZone::Battlefield
                            && self.window.cards().phased_out_under(object.id()).is_none()
                            && evaluated
                                .get(object.id())
                                .is_some_and(|card| card.controller == Some(player))
                    }
                    crate::program::SourceZoneFilter::AnyBattlefield => {
                        object.zone() == ObjectZone::Battlefield
                            && self.window.cards().phased_out_under(object.id()).is_none()
                    }
                    crate::program::SourceZoneFilter::Exile => {
                        object.card_id().is_some() && object.zone() == ObjectZone::Exile
                    }
                    crate::program::SourceZoneFilter::OpponentGraveyard => {
                        object.card_id().is_some()
                            && matches!(object.zone(), ObjectZone::Graveyard(owner) if owner != player)
                    }
                    crate::program::SourceZoneFilter::AnyGraveyard => {
                        object.card_id().is_some()
                            && matches!(object.zone(), ObjectZone::Graveyard(_))
                    }
                    crate::program::SourceZoneFilter::YourLibrary => {
                        object.card_id().is_some() && object.zone() == ObjectZone::Library(player)
                    }
                    crate::program::SourceZoneFilter::OpponentExile => {
                        object.card_id().is_some()
                            && object.zone() == ObjectZone::Exile
                            && object.owner() != player
                    }
                    crate::program::SourceZoneFilter::LinkedExile => {
                        object.card_id().is_some() && object.zone() == ObjectZone::Exile
                    }
                    crate::program::SourceZoneFilter::LastMoved => {
                        last_moved.contains(&object.id())
                    }
                    crate::program::SourceZoneFilter::TargetPlayerHand => {
                        object.card_id().is_some()
                            && self
                                .resolving_player(crate::program::ResolvingPlayer::TargetPlayer)
                                .is_ok_and(|seat| object.zone() == ObjectZone::Hand(seat))
                    }
                    crate::program::SourceZoneFilter::TargetPlayerGraveyard => {
                        object.card_id().is_some()
                            && self
                                .resolving_player(crate::program::ResolvingPlayer::TargetPlayer)
                                .is_ok_and(|seat| object.zone() == ObjectZone::Graveyard(seat))
                    }
                    crate::program::SourceZoneFilter::OpponentHand => {
                        object.card_id().is_some()
                            && matches!(object.zone(), ObjectZone::Hand(owner) if owner != player)
                    }
                    crate::program::SourceZoneFilter::CommandZone => {
                        object.card_id().is_some()
                            && object.zone() == ObjectZone::Command
                            && object.owner() == player
                    }
                };
                zone_ok
                    && evaluated.get(object.id()).is_some_and(|card| {
                        self.matches_object_filter(
                            **filter,
                            card,
                            player,
                            ability_source,
                            excluded,
                            evaluated,
                        )
                    })
            }
            ResolutionSelectionOperation::SacrificeColored => {
                object.zone() == ObjectZone::Battlefield
                    && evaluated.get(object.id()).is_some_and(|v| {
                        v.controller == Some(player) && !v.current_colors.is_empty()
                    })
            }
            ResolutionSelectionOperation::OtherControlledCreature { source, .. } => {
                object.id() != *source
                    && object.zone() == ObjectZone::Battlefield
                    && evaluated.get(object.id()).is_some_and(|value| {
                        value.controller == Some(player) && value.is_creature()
                    })
            }
            ResolutionSelectionOperation::SacrificeMatching { filter, source } => {
                object.zone() == ObjectZone::Battlefield
                    && evaluated.get(object.id()).is_some_and(|value| {
                        value.controller == Some(player) && filter.matches(value, *source)
                    })
            }
            ResolutionSelectionOperation::ExileChosen { filter }
            | ResolutionSelectionOperation::ReturnToHand { filter }
            | ResolutionSelectionOperation::Sacrifice { filter } => {
                object.zone() == ObjectZone::Battlefield
                    && evaluated.get(object.id()).is_some_and(|value| {
                        value.controller == Some(player)
                            && filter.matches_view(value, excluded, mana_value)
                    })
            }
            ResolutionSelectionOperation::TapOrUntapControlled { tap, filter } => {
                object.zone() == ObjectZone::Battlefield
                    && object.tapped() == Some(!*tap)
                    && evaluated.get(object.id()).is_some_and(|view| {
                        view.controller == Some(player)
                            && filter.matches_static_characteristics(view, excluded, mana_value)
                    })
            }
            ResolutionSelectionOperation::RandomDiscard { object: chosen } => {
                object.id() == *chosen
                    && object.zone() == ObjectZone::Hand(player)
                    && object.card_id().is_some()
            }
            ResolutionSelectionOperation::RandomSacrifice { object: chosen } => {
                object.id() == *chosen
                    && object.zone() == ObjectZone::Battlefield
                    && evaluated
                        .get(object.id())
                        .is_some_and(|sheet| sheet.controller == Some(player))
            }
            ResolutionSelectionOperation::Discard => {
                object.card_id().is_some() && object.zone() == ObjectZone::Hand(player)
            }
        }
    }
}

/// One owner of singleton elision and the next physical graveyard permutation.
pub(super) fn next_graveyard_group(
    groups: &[ResolutionGraveyardGroup],
    orders: &mut Vec<ResolutionAcceptedGraveyardOrder>,
) -> Result<Option<ResolutionGraveyardGroup>, EffectError> {
    if orders.len() > groups.len() {
        return Err(EffectError::InconsistentSelectionWork);
    }
    while let Some(group) = groups.get(orders.len()) {
        if group.entrants.len() > 1 {
            return Ok(Some(group.clone()));
        }
        orders.push(ResolutionAcceptedGraveyardOrder {
            owner: group.owner,
            top_to_bottom: group.entrants.clone(),
        });
    }
    Ok(None)
}
pub(super) fn validate_graveyard_order(
    group: &ResolutionGraveyardGroup,
    order: &[ObjectId],
) -> Result<(), EffectError> {
    let actual: BTreeSet<_> = order.iter().copied().collect();
    let expected: BTreeSet<_> = group.entrants.iter().copied().collect();
    if order.len() != group.entrants.len() || actual.len() != order.len() || actual != expected {
        return Err(EffectError::InvalidGraveyardPermutation);
    }
    Ok(())
}
