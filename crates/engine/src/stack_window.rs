//! Native individual-turn stack/priority boundary (CR 117, 405, 608).
//!
//! These are trusted post-legality continuations, not player commands. No card
//! program, mana payment/general costs, complete target-legality predicates,
//! state-based actions or trigger discovery/APNAP ordering,
//! general turn actions or persistence is implemented here. GameState's scheduler
//! reuses this window across steps, retaining identities. The eventual host scopes
//! them to a game and serializes accepted successors.
#[cfg(test)]
mod control_tests;
#[cfg(test)]
pub(crate) mod private_qualification;
use crate::activated::SourceCost;
use crate::shuffle::{EntropyTape, ShuffleRecord};
use crate::{
    TargetId,
    draw::{DrawEvent, DrawJournal},
    object_identity::{
        ControllerChange, GameObject, GameObjects, ObjectDestination, ObjectId,
        ObjectIdentityError, ObjectMovement, ObjectStatusChange, ObjectZone,
    },
    priority::{PriorityError, PriorityPhase, PriorityRound, SeatId},
    targets::{
        ResolutionPermission, TargetAssessment, TargetCheck, TargetChoice, TargetClause,
        TargetError, TargetLegality, TargetPlan, TargetRef, TargetWorld,
    },
};
use spellwake_contracts::RulesInteger;

use std::collections::{BTreeMap, BTreeSet};

/// One stack entry, including abilities that have no associated physical card.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct StackEntryId(u64);

impl StackEntryId {
    /// Synthetic creator id for non-stack permanent entry replacement effects (e.g. lands entering with keyword counters).
    pub(crate) const fn entry_replacement_creator() -> Self {
        Self(0)
    }
}

/// Minimal source evidence only, not a complete ability program or last-known state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StackKind {
    /// The source-free triggered ability a player has from the speed rules.
    Speed,
    /// One of the source-free inherent monarch triggers.
    Monarch,
    /// Source-independent delayed trigger created by a resolving spell copy.
    DelayedCopy {
        original: StackEntryId,
    },
    /// Triggered ability whose actual source is a command-zone emblem.
    Emblem {
        emblem: crate::emblem::EmblemId,
    },
    CardSpell(ObjectId),
    /// A nonpermanent spell copy has no physical CardId. Payload is owned by the executor.
    SpellCopy {
        copied_from: StackEntryId,
        owner: SeatId,
    },
    ActivatedAbility {
        source: GameObject,
    },
    TriggeredAbility {
        source: GameObject,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StackEntry {
    /// Earlier controllers, from admission through subsequent control effects.
    previous_controllers: Vec<SeatId>,
    ability_copy_of: Option<StackEntryId>,
    chosen_x: Option<RulesInteger>,
    caster: Option<SeatId>,
    cast_from_hand: Option<SeatId>,
    pub(crate) cast_from_graveyard: bool,
    pub(crate) cast_original_object: Option<ObjectId>,
    pub(crate) cast_from_command: bool,
    pub(crate) cast_from_library: Option<SeatId>,
    cast_during_own_main: bool,
    protected_by_spent_mana: bool,
    power_contributors: Vec<ObjectId>,
    attacking_defender: Option<SeatId>,
    exile_cast: Option<ExileCast>,
    conditional_cost: bool,
    bargained: bool,
    buyback: bool,
    kicked: BTreeMap<usize, RulesInteger>,
    replicated: BTreeMap<usize, RulesInteger>,
    pub(crate) granted_replicate: BTreeMap<crate::characteristics::EffectInstanceKey, usize>,
    sunburst_colors: u8,
    pub(crate) cast_summary: crate::mana::CastManaSummary,
    id: StackEntryId,
    controller: SeatId,
    kind: StackKind,
    targets: TargetPlan,
}
/// A casting permission which exiles the physical spell whenever it leaves the stack.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ExileCast {
    Flashback,
    JumpStart,
    Aftermath,
    IfGraveyard,
}
impl ExileCast {
    fn destination(self, proposed: ObjectDestination) -> ObjectDestination {
        if self != Self::IfGraveyard || proposed == ObjectDestination::Graveyard {
            ObjectDestination::Exile
        } else {
            proposed
        }
    }
}
impl StackEntry {
    /// Original stack entry for an ability copy. The copy retains the ability’s source.
    pub fn ability_copy_of(&self) -> Option<StackEntryId> {
        self.ability_copy_of
    }
    /// Chosen X is shared by every X symbol on this spell and its copies.
    pub fn chosen_x(&self) -> Option<&RulesInteger> {
        self.chosen_x.as_ref()
    }
    pub(crate) fn cast_summary(&self) -> &crate::mana::CastManaSummary {
        &self.cast_summary
    }
    pub(crate) fn cast_from_any_hand(&self) -> bool {
        self.cast_from_hand.is_some()
    }
    pub(crate) fn cast_origin(&self) -> Option<(SeatId, bool)> {
        self.caster
            .map(|caster| (caster, self.cast_from_hand == Some(caster)))
    }
    pub(crate) fn controller_cast_during_own_main(&self) -> bool {
        self.cast_during_own_main
            && self.caster == Some(self.controller)
            && matches!(self.kind, StackKind::CardSpell(_))
    }
    /// Whether this specific physical spell paid with mana that prevents countering it.
    pub fn protected_by_spent_mana(&self) -> bool {
        self.protected_by_spent_mana
    }
    pub(crate) fn cast_from_hand_of(&self, player: SeatId) -> bool {
        self.cast_from_hand == Some(player)
    }
    pub(crate) fn cast_from_controllers_hand(&self) -> bool {
        self.cast_from_hand == Some(self.controller)
    }
    /// The defender of the unblocked attacker returned to pay sneak or ninjutsu.
    pub fn attacking_defender(&self) -> Option<SeatId> {
        self.attacking_defender
    }
    /// Exact creatures tapped to pay for this crew or saddle activation.
    pub fn power_contributors(&self) -> &[ObjectId] {
        &self.power_contributors
    }

    /// Number of distinct colors actually spent on a spell with sunburst.
    pub fn sunburst_colors(&self) -> u8 {
        self.sunburst_colors
    }
    /// Whether the spell used its printed conditional price, including copied choices.
    pub fn paid_conditional_cost(&self) -> bool {
        self.conditional_cost
    }
    /// Whether buyback was paid, including copied casting decisions (CR 707.10).
    pub fn bought_back(&self) -> bool {
        self.buyback
    }
    /// Whether an eligible permanent was sacrificed to bargain, including on a copied spell.
    pub fn bargained(&self) -> bool {
        self.bargained
    }
    /// The kicker costs paid while announcing this spell.
    pub fn replicate_choices(&self) -> &BTreeMap<usize, RulesInteger> {
        &self.replicated
    }
    pub fn kicker_choices(&self) -> &BTreeMap<usize, RulesInteger> {
        &self.kicked
    }
    /// True only on the physical spell cast by paying its flashback cost.
    pub fn cast_with_flashback(&self) -> bool {
        self.exile_cast == Some(ExileCast::Flashback)
    }
    /// True only on the physical spell cast using jump-start.
    pub fn exiles_if_put_into_graveyard(&self) -> bool {
        self.exile_cast == Some(ExileCast::IfGraveyard)
    }
    pub fn cast_with_jump_start(&self) -> bool {
        self.exile_cast == Some(ExileCast::JumpStart)
    }
    pub fn id(&self) -> StackEntryId {
        self.id
    }
    pub fn controller(&self) -> SeatId {
        self.controller
    }
    pub fn targets(&self) -> &TargetPlan {
        &self.targets
    }
    pub fn kind(&self) -> &StackKind {
        &self.kind
    }
}

/// One final authorized public destination for a selected library card.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct LibraryTopMove {
    pub before: ObjectId,
    pub destination: crate::program::PublicZoneDestination,
}
/// Borrowed rules-authorized partition. This grants no public wire/movement authority.
pub(crate) struct LibraryTopCommit<'a> {
    pub player: SeatId,
    pub frozen: &'a [ObjectId],
    pub top: &'a [ObjectId],
    pub bottom: &'a [ObjectId],
    pub moves_application: &'a [LibraryTopMove],
}
/// A transient non-card spell object. Its key tracks one copy's lifetime, not a
/// physical card or a targetable object outside the stack. Permanent copies and
/// replacement-adjusted destinations are unsupported in this narrow slice.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpellCopyObject {
    /// Stack or departed incarnation of a noncard permanent spell.
    pub permanent_object: Option<ObjectId>,
    pub id: StackEntryId,
    pub owner: SeatId,
    pub controller: Option<SeatId>,
    pub zone: ObjectZone,
}
/// A nonmana, stack-using trigger whose controller/source and APNAP position the kernel has
/// already established. Vector order is bottom-to-top, never chosen by this module.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OrderedTrigger {
    pub controller: SeatId,
    pub source: GameObject,
}

/// Actual allocation identities paired to the supplied bottom-to-top triggers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct OrderedTriggerReceipt {
    window: StackWindow,
    entries: Vec<StackEntryId>,
}
impl OrderedTriggerReceipt {
    pub(crate) fn into_parts(self) -> (StackWindow, Vec<StackEntryId>) {
        (self.window, self.entries)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowPhase {
    Opening,
    /// Mandatory turn work; players have no priority at this boundary.
    TurnWork,
    Priority(PriorityPhase),
    Resolving(StackEntryId),
    CheckingTargets(StackEntryId),
    AllTargetsIllegal(StackEntryId),
    StepEnded,
}
#[derive(Debug, Clone, PartialEq, Eq)]
enum Status {
    Opening,
    TurnWork,
    Priority(PriorityRound),
    // Keep a continuation snapshot even if the resolving spell leaves the stack.
    Resolving {
        entry: StackEntry,
        targets: TargetAssessment,
    },
    CheckingTargets(StackEntry),
    AllTargetsIllegal {
        entry: StackEntry,
        targets: TargetAssessment,
    },
    StepEnded,
}

/// Immutable bookkeeping for one stable individual-turn step and its participants.
/// Stack entries remain present during resolution until their rule-defined exit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StackWindow {
    revision: u64,
    next_entry: u64,
    order: Vec<SeatId>,
    original_order: Vec<SeatId>,
    active: SeatId,
    cards: GameObjects,
    stack: Vec<StackEntry>,
    copies: BTreeMap<StackEntryId, SpellCopyObject>,
    status: Status,
}

/// One local draw call, borrowing its unchanged accepted basis. It is never
/// retained in pending work. Only completion produces an adoptable successor.
#[derive(Debug)]
pub(crate) struct DrawBatch<'a> {
    basis: &'a StackWindow,
    revision: u64,
    cards: GameObjects,
    player: SeatId,
    remaining: RulesInteger,
    journal: DrawJournal,
}

/// A real physical cut, or the complete batch receipt. The predecessor registry
/// lives only in this cut; occurrence discovery must happen before advancing next.
// Transient single-use return enum; keeping `StackWindow` inline avoids extra heap churn per draw batch.
#[allow(clippy::large_enum_variant)]
#[derive(Debug)]
pub(crate) enum DrawBatchProgress<'a> {
    Drawn {
        before: GameObjects,
        next: DrawBatch<'a>,
        movement: ObjectMovement,
    },
    Complete {
        window: StackWindow,
        journal: DrawJournal,
    },
}
impl<'a> DrawBatch<'a> {
    pub(crate) fn cards(&self) -> &GameObjects {
        &self.cards
    }
    /// Consuming the local batch ensures an error cannot leave a finishable
    /// tentative prefix. Nothing is published outside the enclosing transaction.
    pub(crate) fn next_draw(mut self) -> Result<DrawBatchProgress<'a>, StackWindowError> {
        if self.remaining > 0.into() {
            let before = self.cards().clone();
            if let Some(movement) = self.cards.draw_card_in_place(self.player)? {
                self.journal.record_draw(self.player, &movement);
                self.remaining = self.remaining.minus(&1.into());
                return Ok(DrawBatchProgress::Drawn {
                    before,
                    next: self,
                    movement,
                });
            }
            self.journal.events.push(DrawEvent::EmptyLibrary {
                player: self.player,
                attempts: self.remaining,
            });
        }
        Ok(DrawBatchProgress::Complete {
            window: self.basis.with_cards(self.revision, self.cards),
            journal: self.journal,
        })
    }
}

/// An actual exit produced by the single stack/card mutator, never inferred by CardId.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum StackExit {
    Object(ObjectMovement),
    Copy {
        before: SpellCopyObject,
        after: SpellCopyObject,
    },
}

/// Atomic targeted ability admission and physical source-cost result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaidActivation {
    state: StackWindow,
    entry: StackEntryId,
    source_before: GameObject,
    source_after: GameObject,
}
impl PaidActivation {
    pub fn state(&self) -> &StackWindow {
        &self.state
    }
    pub fn entry(&self) -> StackEntryId {
        self.entry
    }
    pub fn source_before(&self) -> &GameObject {
        &self.source_before
    }
    pub fn source_after(&self) -> &GameObject {
        &self.source_after
    }
    pub fn into_state(self) -> StackWindow {
        self.state
    }
}
/// Native rejection or unsupported integration, not an adjudicated game outcome.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StackWindowError {
    NotOwnHand,
    NotInTurnWork,
    StaleRevision,
    RevisionExhausted,
    EntryIdentityExhausted,
    WrongController,
    SourceTapped,
    UnknownPlayer,
    UntrackedInitialStack,
    Priority(PriorityError),
    Objects(ObjectIdentityError),
    NotInPriority,
    NotResolving,
    WrongResolution,
    ChecksNotPending,
    EmptyTriggerBatch,
    UnsupportedStackInsertion,
    MissingSpellExit,
    UnexpectedSpellExit,
    InconsistentStack,
    Targets(TargetError),
    TargetChecksNotPending,
    TargetsNotRejected,
    UnknownTarget,
    IllegalTarget,
    UnsupportedTargetOperation,
    EmptyCheckMoves,
    InvalidCheckMove,
    /// The resolving batch wrapper requires at least one real selected movement.
    EmptyResolutionMoves,
    /// A selected identity, physical origin, zone, controller or uniqueness failed.
    InvalidResolutionMove,
    InvalidLibraryTop,
    NotSpell,
    CopyChecksRequired,
}
/// Checked physical source conditions for an effect-time selected-object batch.
/// Type predicates stay in the characteristic executor, not this registry owner.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ResolutionSelectionMove {
    GraveyardReturn {
        player: SeatId,
        object: ObjectId,
        destination: crate::program::PublicZoneDestination,
    },
    BattlefieldEffect {
        player: SeatId,
        object: ObjectId,
        destination: crate::program::PublicZoneDestination,
    },
    Sacrifice {
        player: SeatId,
        object: ObjectId,
        destination: crate::program::PublicZoneDestination,
    },
    FromHand {
        player: SeatId,
        object: ObjectId,
        destination: crate::program::PublicZoneDestination,
    },
}
impl From<PriorityError> for StackWindowError {
    fn from(error: PriorityError) -> Self {
        Self::Priority(error)
    }
}
impl From<ObjectIdentityError> for StackWindowError {
    fn from(error: ObjectIdentityError) -> Self {
        Self::Objects(error)
    }
}

impl From<TargetError> for StackWindowError {
    fn from(error: TargetError) -> Self {
        Self::Targets(error)
    }
}

impl TargetWorld for StackWindow {
    fn object_zone(&self, card: ObjectId) -> Option<ObjectZone> {
        self.cards.get(card).map(GameObject::zone)
    }
    fn player_exists(&self, player: SeatId) -> bool {
        self.order.contains(&player)
    }
    fn stack_exists(&self, entry: StackEntryId) -> bool {
        self.stack
            .iter()
            .chain(self.resolving())
            .any(|item| item.id == entry)
    }
    fn stack_precedes(&self, target: StackEntryId, source: StackEntryId) -> bool {
        match (
            self.stack.iter().position(|item| item.id == target),
            self.stack.iter().position(|item| item.id == source),
        ) {
            (Some(target), Some(source)) => target < source,
            _ => false,
        }
    }
}

impl StackWindow {
    pub(crate) fn retain_attack_object_mark(&mut self, mark: [u64; 2]) {
        self.cards.retain_attack_mark(mark);
    }
    pub(crate) fn attack_allocation_mark(&self) -> [u64; 2] {
        [self.revision, self.next_entry]
    }
    pub(crate) fn retain_attack_mark(&mut self, mark: [u64; 2]) {
        self.revision = self.revision.max(mark[0]);
        self.next_entry = self.next_entry.max(mark[1]);
    }

    /// Begin at a caller-selected step boundary after turn-based work. No player
    /// gets priority until the enclosing kernel settles CR 117.5 and finishes checks.
    /// Existing stack objects need a future explicit restoration contract, not inference.
    pub fn new(
        order: Vec<SeatId>,
        active: SeatId,
        cards: GameObjects,
    ) -> Result<Self, StackWindowError> {
        let round = PriorityRound::new(order.clone(), active, 0)?;
        for card in cards.iter() {
            if !order.contains(&card.owner())
                || card.controller().is_some_and(|seat| !order.contains(&seat))
            {
                return Err(StackWindowError::UnknownPlayer);
            }
            if card.zone() == ObjectZone::Stack {
                return Err(StackWindowError::UntrackedInitialStack);
            }
        }
        Ok(Self {
            revision: 0,
            next_entry: 1,
            original_order: order.clone(),
            order,
            active,
            cards,
            stack: Vec::new(),
            copies: BTreeMap::new(),
            status: Status::Priority(round),
        })
    }
    pub fn revision(&self) -> u64 {
        self.revision
    }
    /// Privileged operational counter; allocation receipts identify actual entries.
    pub fn next_entry_sequence(&self) -> u64 {
        self.next_entry
    }
    /// Actual live priority owner's revision, when a priority round exists.
    pub fn priority_revision(&self) -> Option<u64> {
        match &self.status {
            Status::Priority(round) => Some(round.revision()),
            _ => None,
        }
    }
    /// Cross-module overflow fault injection, absent from production builds.
    #[cfg(test)]
    pub(crate) fn set_test_revision(&mut self, revision: u64) {
        self.revision = revision;
    }
    #[cfg(test)]
    pub(crate) fn set_test_next_entry(&mut self, next_entry: u64) {
        self.next_entry = next_entry;
    }
    /// Current native participants; account/session authority is outside this store.
    pub fn participants(&self) -> &[SeatId] {
        &self.order
    }
    /// The seat whose turn began, even if it has since left the game.
    pub fn active_player(&self) -> SeatId {
        self.active
    }
    /// The rules-active participant, absent for the rest of a departed player's turn.
    pub fn active_participant(&self) -> Option<SeatId> {
        self.order.contains(&self.active).then_some(self.active)
    }
    /// Fixed turn order, including departed seats, for rotation and result provenance.
    pub fn original_participants(&self) -> &[SeatId] {
        &self.original_order
    }
    pub(crate) fn original_turn_order_from(
        &self,
        seat: SeatId,
    ) -> Result<impl Iterator<Item = SeatId> + '_, StackWindowError> {
        let index = self
            .original_order
            .iter()
            .position(|player| *player == seat)
            .ok_or(StackWindowError::UnknownPlayer)?;
        Ok(self
            .original_order
            .iter()
            .cycle()
            .skip(index)
            .take(self.original_order.len())
            .copied())
    }
    pub(crate) fn next_participant_after(&self, seat: SeatId) -> Result<SeatId, StackWindowError> {
        let index = self
            .original_order
            .iter()
            .position(|player| *player == seat)
            .ok_or(StackWindowError::UnknownPlayer)?;
        self.original_order
            .iter()
            .cycle()
            .skip(index + 1)
            .take(self.original_order.len())
            .find(|player| self.order.contains(player))
            .copied()
            .ok_or(StackWindowError::UnknownPlayer)
    }
    fn step_priority_candidate(&self) -> Result<SeatId, StackWindowError> {
        match self.active_participant() {
            Some(active) => Ok(active),
            None => self.next_participant_after(self.active),
        }
    }
    pub(crate) fn remove_departed_owners(
        &mut self,
        players: &BTreeSet<SeatId>,
    ) -> Result<(), StackWindowError> {
        self.cards.remove_departed_owners(players)?;
        Ok(())
    }

    /// Remove departed players and their stack objects while keeping the turn's original seat.
    pub(crate) fn remove_departed_players(
        &mut self,
        players: &BTreeSet<SeatId>,
        exile: &[(ObjectId, ObjectDestination)],
    ) -> Result<Vec<ObjectMovement>, StackWindowError> {
        let revision = self.next_revision(self.revision)?;
        self.order.retain(|seat| !players.contains(seat));
        self.cards.remove_departed_stack_copies(players);
        self.stack.retain(|entry| match entry.kind() {
            StackKind::CardSpell(id) => {
                self.cards.get(*id).is_some() && !players.contains(&entry.controller())
            }
            StackKind::SpellCopy { owner, .. } => {
                !players.contains(owner) && !players.contains(&entry.controller())
            }
            _ => !players.contains(&entry.controller()),
        });
        self.copies.retain(|_, copy| {
            !players.contains(&copy.owner)
                && copy.controller.is_none_or(|seat| !players.contains(&seat))
        });
        self.status = match &self.status {
            Status::Opening => {
                // No turn has started yet. Keep the first live seat as the starter.
                self.active = self.step_priority_candidate()?;
                Status::Opening
            }
            Status::Priority(round) => {
                Status::Priority(round.after_departures(self.order.clone())?)
            }
            Status::Resolving { entry, targets } => Status::Resolving {
                // CR 608.2m retains an already resolving instruction spell/ability even
                // when CR 800.4a removed its actual object from the stack.
                entry: self
                    .stack
                    .iter()
                    .find(|current| current.id() == entry.id())
                    .unwrap_or(entry)
                    .clone(),
                targets: targets.clone(),
            },
            Status::TurnWork | Status::StepEnded => self.status.clone(),
            Status::CheckingTargets(_) | Status::AllTargetsIllegal { .. } => {
                return Err(StackWindowError::ChecksNotPending);
            }
        };
        let (next, changes) = self.apply_recorded_zone_moves(revision, exile)?;
        *self = next;
        Ok(changes)
    }
    /// Actual stable individual-turn participant order, starting at the active
    /// player. Placement and simultaneous graveyard grouping share this traversal.
    pub(crate) fn apnap_players(
        &self,
    ) -> Result<impl Iterator<Item = SeatId> + '_, StackWindowError> {
        let first = self.step_priority_candidate()?;
        let start = self
            .order
            .iter()
            .position(|player| *player == first)
            .ok_or(StackWindowError::UnknownPlayer)?;
        Ok(self
            .order
            .iter()
            .cycle()
            .skip(start)
            .take(self.order.len())
            .copied())
    }
    pub(crate) fn phase_out(
        &mut self,
        objects: &[ObjectId],
        attachments: &BTreeMap<ObjectId, TargetRef>,
    ) -> Result<Vec<ObjectId>, StackWindowError> {
        Ok(self.cards.phase_out(objects, attachments)?)
    }

    pub(crate) fn phase_at_untap(
        &mut self,
        roots: &[ObjectId],
        skipped_turns: &BTreeSet<SeatId>,
        attachments: &BTreeMap<ObjectId, TargetRef>,
    ) -> Result<(Vec<ObjectId>, Vec<ObjectId>), StackWindowError> {
        Ok(self
            .cards
            .phase_at_untap(self.active, roots, skipped_turns, attachments)?)
    }

    pub fn cards(&self) -> &GameObjects {
        &self.cards
    }
    /// Canonical internal entries, bottom-to-top. Not a seat-safe projection.
    pub fn entries(&self) -> &[StackEntry] {
        &self.stack
    }
    /// Canonical non-card copy objects, including exited copies pending SBA.
    pub fn spell_copies(&self) -> &BTreeMap<StackEntryId, SpellCopyObject> {
        &self.copies
    }
    pub fn spell_owner(&self, entry: &StackEntry) -> Option<SeatId> {
        match entry.kind() {
            StackKind::CardSpell(object) => self.cards.get(*object).map(GameObject::owner),
            StackKind::SpellCopy { owner, .. } => Some(*owner),
            _ => None,
        }
    }
    pub fn phase(&self) -> WindowPhase {
        match &self.status {
            Status::Opening => WindowPhase::Opening,
            Status::TurnWork => WindowPhase::TurnWork,
            Status::Priority(round) => WindowPhase::Priority(round.phase()),
            Status::Resolving { entry, .. } => WindowPhase::Resolving(entry.id),
            Status::CheckingTargets(entry) => WindowPhase::CheckingTargets(entry.id),
            Status::AllTargetsIllegal { entry, .. } => WindowPhase::AllTargetsIllegal(entry.id),
            Status::StepEnded => WindowPhase::StepEnded,
        }
    }
    pub fn resolving(&self) -> Option<&StackEntry> {
        match &self.status {
            Status::Resolving { entry, .. } => Some(entry),
            _ => None,
        }
    }
    fn next_revision(&self, expected: u64) -> Result<u64, StackWindowError> {
        if expected != self.revision {
            return Err(StackWindowError::StaleRevision);
        }
        self.revision
            .checked_add(1)
            .ok_or(StackWindowError::RevisionExhausted)
    }
    /// Replace the canonical registry without cloning the registry being replaced.
    fn with_cards(&self, revision: u64, cards: GameObjects) -> Self {
        Self {
            revision,
            next_entry: self.next_entry,
            order: self.order.clone(),
            original_order: self.original_order.clone(),
            active: self.active,
            cards,
            stack: self.stack.clone(),
            copies: self.copies.clone(),
            status: self.status.clone(),
        }
    }
    /// Apply one already-evaluated battlefield control cut. All validation and
    /// mutation stay on the tentative registry; failures publish no prefix.
    pub(crate) fn reconcile_controllers(
        &self,
        expected: u64,
        controllers: &BTreeMap<ObjectId, SeatId>,
    ) -> Result<(Self, Vec<ControllerChange>), StackWindowError> {
        let revision = self.next_revision(expected)?;
        for (&object, &controller) in controllers {
            self.validate_player(controller)?;
            if self
                .cards
                .get(object)
                .ok_or(ObjectIdentityError::UnknownObject)?
                .zone()
                != ObjectZone::Battlefield
            {
                return Err(ObjectIdentityError::NotPermanent.into());
            }
        }
        let (cards, changes) = self
            .cards
            .apply_effective_controllers(self.cards.revision(), controllers)?;
        Ok((self.with_cards(revision, cards), changes))
    }
    /// Native scheduler continuation. Preserve every object/entry allocator;
    /// only a completed step, turn work, or untouched setup can open a boundary.
    pub(crate) fn enter_turn_step(
        &self,
        expected: u64,
        active: SeatId,
        priority: bool,
    ) -> Result<Self, StackWindowError> {
        let revision = self.next_revision(expected)?;
        let initial = self.revision == 0
            && matches!(
                self.phase(),
                WindowPhase::Priority(PriorityPhase::RequiresChecks { .. })
            );
        if !self.stack.is_empty()
            || !(initial
                || matches!(
                    self.status,
                    Status::Opening | Status::StepEnded | Status::TurnWork
                ))
        {
            return Err(StackWindowError::NotInTurnWork);
        }
        if active != self.active && !self.order.contains(&active) {
            return Err(StackWindowError::UnknownPlayer);
        }
        let status = if priority {
            let first = if self.order.contains(&active) {
                active
            } else {
                self.next_participant_after(active)?
            };
            Status::Priority(PriorityRound::new(self.order.clone(), first, 0)?)
        } else {
            Status::TurnWork
        };
        Ok(Self {
            revision,
            active,
            status,
            ..self.clone()
        })
    }
    /// Trusted actual turn-start continuation, independent from the untap action.
    pub(crate) fn record_turn_start(&self, expected: u64) -> Result<Self, StackWindowError> {
        let revision = self.next_revision(expected)?;
        if !self.stack.is_empty() || !matches!(self.status, Status::TurnWork) {
            return Err(StackWindowError::NotInTurnWork);
        }
        if !self.order.contains(&self.active) {
            return Err(StackWindowError::UnknownPlayer);
        }
        Ok(Self {
            revision,
            cards: self
                .cards
                .record_turn_start(self.cards.revision(), self.active)?,
            ..self.clone()
        })
    }
    pub(crate) fn begin_opening(&self, expected: u64) -> Result<Self, StackWindowError> {
        let revision = self.next_revision(expected)?;
        if self.revision != 0 || !self.stack.is_empty() {
            return Err(StackWindowError::NotInTurnWork);
        }
        Ok(Self {
            revision,
            status: Status::Opening,
            ..self.clone()
        })
    }
    pub(crate) fn opening_shuffle(
        &self,
        expected: u64,
        player: SeatId,
        entropy: &EntropyTape,
    ) -> Result<(Self, EntropyTape, ShuffleRecord), StackWindowError> {
        let revision = self.next_revision(expected)?;
        if self.phase() != WindowPhase::Opening {
            return Err(StackWindowError::NotInTurnWork);
        }
        if !self.order.contains(&player) {
            return Err(StackWindowError::UnknownPlayer);
        }
        let shuffled = self
            .cards
            .shuffle_library(self.cards.revision(), player, entropy)?;
        Ok((
            Self {
                revision,
                cards: shuffled.next,
                ..self.clone()
            },
            shuffled.entropy,
            shuffled.record,
        ))
    }
    pub(crate) fn search_shuffle(
        &self,
        player: SeatId,
        entropy: &EntropyTape,
    ) -> Result<(Self, EntropyTape, ShuffleRecord), StackWindowError> {
        if !matches!(self.status, Status::Resolving { .. }) {
            return Err(StackWindowError::NotResolving);
        }
        let revision = self.next_revision(self.revision)?;
        let shuffled = self
            .cards
            .shuffle_library(self.cards.revision(), player, entropy)?;
        Ok((
            Self {
                revision,
                cards: shuffled.next,
                ..self.clone()
            },
            shuffled.entropy,
            shuffled.record,
        ))
    }
    pub(crate) fn search_shuffle_to_top(
        &self,
        player: SeatId,
        selected: Option<ObjectId>,
        entropy: &EntropyTape,
    ) -> Result<(Self, EntropyTape, ShuffleRecord), StackWindowError> {
        if !matches!(self.status, Status::Resolving { .. }) {
            return Err(StackWindowError::NotResolving);
        }
        let revision = self.next_revision(self.revision)?;
        let shuffled = self.cards.shuffle_library_with_top(
            self.cards.revision(),
            player,
            selected,
            entropy,
        )?;
        Ok((
            Self {
                revision,
                cards: shuffled.next,
                ..self.clone()
            },
            shuffled.entropy,
            shuffled.record,
        ))
    }
    pub(crate) fn opening_bottom_with_movements(
        &self,
        expected: u64,
        player: SeatId,
        objects: &[ObjectId],
    ) -> Result<(Self, Vec<crate::object_identity::ObjectMovement>), StackWindowError> {
        let revision = self.next_revision(expected)?;
        if self.phase() != WindowPhase::Opening {
            return Err(StackWindowError::NotInTurnWork);
        }
        if !self.order.contains(&player) {
            return Err(StackWindowError::UnknownPlayer);
        }
        let mut next = self.clone();
        let mut movements = Vec::with_capacity(objects.len());
        for &object in objects {
            if next.cards.get(object).map(GameObject::zone) != Some(ObjectZone::Hand(player)) {
                return Err(StackWindowError::InvalidCheckMove);
            }
            let moved = next
                .cards
                .apply_zone_move(
                    next.cards.revision(),
                    object,
                    ObjectDestination::LibraryBottom,
                )?
                .require_moved()?;
            let before = moved.before.ok_or(StackWindowError::InvalidCheckMove)?;
            movements.push(crate::object_identity::ObjectMovement {
                before,
                after: moved.after,
            });
            next.cards = moved.next;
        }
        next.revision = revision;
        Ok((next, movements))
    }

    pub(crate) fn opening_battlefield_with_movements(
        &self,
        expected: u64,
        player: SeatId,
        objects: &[ObjectId],
    ) -> Result<(Self, Vec<crate::object_identity::ObjectMovement>), StackWindowError> {
        let revision = self.next_revision(expected)?;
        if self.phase() != WindowPhase::Opening {
            return Err(StackWindowError::NotInTurnWork);
        }
        if !self.order.contains(&player) {
            return Err(StackWindowError::UnknownPlayer);
        }
        let mut next = self.clone();
        let mut movements = Vec::with_capacity(objects.len());
        for &object in objects {
            if next.cards.get(object).map(GameObject::zone) != Some(ObjectZone::Hand(player)) {
                return Err(StackWindowError::InvalidCheckMove);
            }
            let moved = next
                .cards
                .apply_zone_move(
                    next.cards.revision(),
                    object,
                    ObjectDestination::Battlefield { controller: player },
                )?
                .require_moved()?;
            let before = moved.before.ok_or(StackWindowError::InvalidCheckMove)?;
            movements.push(crate::object_identity::ObjectMovement {
                before,
                after: moved.after,
            });
            next.cards = moved.next;
        }
        next.revision = revision;
        Ok((next, movements))
    }
    /// Receipt-dropping primitive adapter; executor publication retains status pairs.
    #[cfg(test)]
    pub(crate) fn untap_active(&self, expected: u64) -> Result<Self, StackWindowError> {
        self.untap_active_with_changes(expected, &BTreeSet::new(), &BTreeSet::new())
            .map(|(state, _)| state)
    }
    pub(crate) fn untap_active_with_changes(
        &self,
        expected: u64,
        excluded: &BTreeSet<ObjectId>,
        additional: &BTreeSet<ObjectId>,
    ) -> Result<(Self, Vec<ObjectStatusChange>), StackWindowError> {
        let revision = self.next_revision(expected)?;
        if !matches!(self.status, Status::TurnWork) {
            return Err(StackWindowError::NotInTurnWork);
        }
        let objects: Vec<_> = self
            .cards
            .iter()
            .filter(|object| {
                (object.controller() == Some(self.active) || additional.contains(&object.id()))
                    && object.tapped() == Some(true)
                    && !excluded.contains(&object.id())
            })
            .map(GameObject::id)
            .collect();
        let (cards, changes) =
            self.cards
                .set_tapped_batch(self.cards.revision(), &objects, false)?;
        Ok((self.with_cards(revision, cards), changes))
    }
    /// Physical attack taps during actual mandatory work. The executor separately
    /// establishes creature, control-history and combat-policy legality.
    pub(crate) fn record_declaration_taps(
        &self,
        expected: u64,
        actor: SeatId,
        objects: &[ObjectId],
    ) -> Result<(Self, Vec<ObjectStatusChange>), StackWindowError> {
        let revision = self.next_revision(expected)?;
        if !self.stack.is_empty() || !matches!(self.status, Status::TurnWork) {
            return Err(StackWindowError::NotInTurnWork);
        }
        self.validate_player(actor)?;
        if actor != self.active {
            return Err(StackWindowError::WrongController);
        }
        for &object in objects {
            let card = self
                .cards
                .get(object)
                .ok_or(ObjectIdentityError::UnknownObject)?;
            if card.zone() != ObjectZone::Battlefield {
                return Err(ObjectIdentityError::NotPermanent.into());
            }
            if card.controller() != Some(actor) {
                return Err(StackWindowError::WrongController);
            }
            if card.tapped() != Some(false) {
                return Err(StackWindowError::SourceTapped);
            }
        }
        let (cards, changes) = self
            .cards
            .set_tapped_batch(self.cards.revision(), objects, true)?;
        Ok((self.with_cards(revision, cards), changes))
    }
    pub(crate) fn reverse_declaration_taps(
        &self,
        actor: SeatId,
        taps: &[crate::combat::AttackTap],
    ) -> Result<Self, StackWindowError> {
        if self.active != actor || self.phase() != WindowPhase::TurnWork {
            return Err(StackWindowError::NotInTurnWork);
        }
        let objects = taps
            .iter()
            .filter_map(|tap| {
                self.cards
                    .get(tap.after.id())
                    .filter(|card| card.tapped() == Some(true))
                    .map(GameObject::id)
            })
            .collect::<Vec<_>>();
        let (cards, _) = self
            .cards
            .set_tapped_batch(self.cards.revision(), &objects, false)?;
        Ok(self.with_cards(self.next_revision(self.revision)?, cards))
    }
    /// Trusted ordinary draw work. A player command cannot select a library or
    /// count; the enclosing scheduler/evaluator establishes the instruction.
    /// Test-only direct mechanism fixture; production uses the shared driver.
    #[cfg(test)]
    pub(crate) fn draw_cards(
        &self,
        expected: u64,
        player: SeatId,
        amount: RulesInteger,
    ) -> Result<(Self, DrawJournal), StackWindowError> {
        let mut batch = self.begin_draw_batch(expected, player, amount)?;
        loop {
            match batch.next_draw()? {
                DrawBatchProgress::Drawn { next, .. } => batch = next,
                DrawBatchProgress::Complete { window, journal } => return Ok((window, journal)),
            }
        }
    }
    /// Reserve the outer revision once, retaining the existing work/player/count
    /// checks even when no physical card will move. Every advance uses the actual
    /// one-card registry primitive, with no intermediate priority or check work.
    pub(crate) fn begin_draw_batch(
        &self,
        expected: u64,
        player: SeatId,
        amount: RulesInteger,
    ) -> Result<DrawBatch<'_>, StackWindowError> {
        let revision = self.validate_draw(expected, player, &amount)?;
        Ok(DrawBatch {
            basis: self,
            revision,
            cards: self.cards.clone(),
            player,
            remaining: amount,
            journal: DrawJournal::default(),
        })
    }
    fn require_checks(&self) -> Result<(), StackWindowError> {
        if matches!(
            self.phase(),
            WindowPhase::TurnWork | WindowPhase::Priority(PriorityPhase::RequiresChecks { .. })
        ) {
            Ok(())
        } else {
            Err(StackWindowError::ChecksNotPending)
        }
    }
    fn priority(&self) -> Result<&PriorityRound, StackWindowError> {
        match &self.status {
            Status::Priority(round) => Ok(round),
            _ => Err(StackWindowError::NotInPriority),
        }
    }
    /// Validate an original draw event before any replacement can suppress the draw.
    pub(crate) fn validate_draw(
        &self,
        expected: u64,
        player: SeatId,
        amount: &RulesInteger,
    ) -> Result<u64, StackWindowError> {
        let revision = self.next_revision(expected)?;
        if !matches!(
            self.phase(),
            WindowPhase::Opening | WindowPhase::TurnWork | WindowPhase::Resolving(_)
        ) {
            return Err(StackWindowError::NotInTurnWork);
        }
        if !self.order.contains(&player) {
            return Err(StackWindowError::UnknownPlayer);
        }
        if amount < &0.into() {
            return Err(ObjectIdentityError::NegativeDrawCount.into());
        }
        Ok(revision)
    }
    /// Check action timing without recording an action or paying a source cost.
    pub(crate) fn validate_action_actor(&self, actor: SeatId) -> Result<(), StackWindowError> {
        self.priority()?.validate_holder(actor)?;
        Ok(())
    }
    fn validate_player(&self, seat: SeatId) -> Result<(), StackWindowError> {
        if self.order.contains(&seat) {
            Ok(())
        } else {
            Err(StackWindowError::UnknownPlayer)
        }
    }
    fn validate_source(&self, source: &GameObject) -> Result<(), StackWindowError> {
        // A captured source can belong to someone who has already left (CR 603.10f).
        if !self.original_order.contains(&source.owner())
            || source
                .controller()
                .is_some_and(|controller| !self.original_order.contains(&controller))
        {
            return Err(StackWindowError::UnknownPlayer);
        }
        Ok(())
    }
    fn allocate(
        &mut self,
        controller: SeatId,
        kind: StackKind,
    ) -> Result<StackEntry, StackWindowError> {
        let next = self
            .next_entry
            .checked_add(1)
            .ok_or(StackWindowError::EntryIdentityExhausted)?;
        let entry = StackEntry {
            previous_controllers: Vec::new(),
            ability_copy_of: None,
            chosen_x: None,
            caster: None,
            cast_from_hand: None,
            cast_original_object: None,
            cast_from_graveyard: false,
            cast_from_command: false,
            cast_from_library: None,
            cast_during_own_main: false,
            protected_by_spent_mana: false,
            power_contributors: Vec::new(),
            attacking_defender: None,
            exile_cast: None,
            conditional_cost: false,
            bargained: false,
            buyback: false,
            kicked: BTreeMap::new(),
            replicated: BTreeMap::new(),
            granted_replicate: BTreeMap::new(),
            sunburst_colors: 0,
            cast_summary: crate::mana::CastManaSummary::default(),
            id: StackEntryId(self.next_entry),
            controller,
            kind,
            targets: TargetPlan::default(),
        };
        self.next_entry = next;
        Ok(entry)
    }
    /// Trusted continuation only after the caller actually settles checks/triggers.
    pub fn finish_checks(&self, expected: u64) -> Result<Self, StackWindowError> {
        let revision = self.next_revision(expected)?;
        let round = self.priority()?;
        if self
            .copies
            .values()
            .any(|copy| copy.zone != ObjectZone::Stack)
        {
            return Err(StackWindowError::CopyChecksRequired);
        }
        let round = round.finish_checks(round.revision())?;
        Ok(Self {
            revision,
            status: self.status_after_passes(round),
            ..self.clone()
        })
    }
    pub fn pass(&self, expected: u64, player: SeatId) -> Result<Self, StackWindowError> {
        let revision = self.next_revision(expected)?;
        let round = self.priority()?;
        let round = round.pass(round.revision(), player)?;
        Ok(Self {
            revision,
            status: self.status_after_passes(round),
            ..self.clone()
        })
    }

    fn status_after_passes(&self, round: PriorityRound) -> Status {
        if round.phase() == PriorityPhase::AllPassed {
            match self.stack.last() {
                Some(entry) if !entry.targets.targets().is_empty() => {
                    Status::CheckingTargets(entry.clone())
                }
                Some(entry) => Status::Resolving {
                    entry: entry.clone(),
                    targets: TargetAssessment::no_targets(),
                },
                None => Status::StepEnded,
            }
        } else {
            Status::Priority(round)
        }
    }
    /// Trusted completed tap activation. Reuse priority action/check bookkeeping,
    /// but do not allocate or insert a stack entry for a mana ability (CR 605.3b).
    pub fn record_tap_action(
        &self,
        expected: u64,
        actor: SeatId,
        source: ObjectId,
    ) -> Result<Self, StackWindowError> {
        let revision = self.next_revision(expected)?;
        let round = self.priority()?;
        let round = round.record_action(round.revision(), actor)?;
        let changed = self.source_cost_change(actor, source, SourceCost::Tap, false)?;
        Ok(Self {
            revision,
            cards: changed.next,
            status: Status::Priority(round),
            ..self.clone()
        })
    }
    /// Reserve a priority mana action whose source will be sacrificed without tapping.
    pub(crate) fn record_mana_action(
        &self,
        expected: u64,
        actor: SeatId,
    ) -> Result<Self, StackWindowError> {
        let revision = self.next_revision(expected)?;
        let round = self.priority()?;
        let round = round.record_action(round.revision(), actor)?;
        Ok(Self {
            revision,
            status: Status::Priority(round),
            ..self.clone()
        })
    }
    /// Reserve a non-tap mana action during an already resolving payment.
    pub(crate) fn record_resolution_mana_action(
        &self,
        expected: u64,
        resolving: StackEntryId,
    ) -> Result<Self, StackWindowError> {
        let revision = self.next_revision(expected)?;
        if self.resolving().map(StackEntry::id) != Some(resolving) {
            return Err(StackWindowError::WrongResolution);
        }
        Ok(Self {
            revision,
            ..self.clone()
        })
    }
    /// Commit a proven source sacrifice without manufacturing a stack entry.
    pub(crate) fn sacrifice_mana_source(
        &self,
        actor: SeatId,
        source: ObjectId,
        destination: ObjectDestination,
    ) -> Result<(Self, GameObject), StackWindowError> {
        let card = self
            .cards
            .get(source)
            .ok_or(ObjectIdentityError::UnknownObject)?;
        if card.zone() != ObjectZone::Battlefield {
            return Err(ObjectIdentityError::NotPermanent.into());
        }
        if card.controller() != Some(actor) {
            return Err(StackWindowError::WrongController);
        }
        if matches!(destination, ObjectDestination::Battlefield { .. }) {
            return Err(StackWindowError::InconsistentStack);
        }
        let change = self
            .cards
            .apply_zone_move(self.cards.revision(), source, destination)?
            .require_moved()?;
        Ok((
            Self {
                cards: change.next,
                ..self.clone()
            },
            change.after,
        ))
    }
    /// Untap the selected battlefield objects at the current resolution cut.
    pub(crate) fn untap_for_effect(
        &self,
        entry: StackEntryId,
        objects: &[ObjectId],
    ) -> Result<(Self, Vec<ObjectStatusChange>), StackWindowError> {
        if self.resolving().map(StackEntry::id) != Some(entry) {
            return Err(StackWindowError::WrongResolution);
        }
        let revision = self.next_revision(self.revision)?;
        let (cards, changes) =
            self.cards
                .set_tapped_batch(self.cards.revision(), objects, false)?;
        Ok((self.with_cards(revision, cards), changes))
    }
    pub(crate) fn tap_for_effect(
        &self,
        entry: StackEntryId,
        objects: &[ObjectId],
    ) -> Result<(Self, Vec<ObjectStatusChange>), StackWindowError> {
        if self.resolving().map(StackEntry::id) != Some(entry) {
            return Err(StackWindowError::WrongResolution);
        }
        let revision = self.next_revision(self.revision)?;
        let (cards, changes) = self
            .cards
            .set_tapped_batch(self.cards.revision(), objects, true)?;
        Ok((self.with_cards(revision, cards), changes))
    }
    /// Tap or untap battlefield objects during an inline event-replacement body (CR 614.1),
    /// which may execute outside stack resolution (e.g. draw step or combat damage lifelink).
    pub(crate) fn record_inline_replacement_status(
        &self,
        objects: &[ObjectId],
        tapped: bool,
    ) -> Result<(Self, Vec<ObjectStatusChange>), StackWindowError> {
        let revision = self.next_revision(self.revision)?;
        let (cards, changes) =
            self.cards
                .set_tapped_batch(self.cards.revision(), objects, tapped)?;
        Ok((self.with_cards(revision, cards), changes))
    }
    /// Trusted resolution-time mana tap after the executor proves payment authority.
    /// This preserves the resolving entry and does not create priority/check work.
    ///
    /// # Errors
    /// Rejects a stale revision, another resolving entry or invalid physical tap
    /// cost. The caller must separately validate the exact mana ability and payer.
    pub(crate) fn record_resolution_tap(
        &self,
        expected: u64,
        resolving: StackEntryId,
        actor: SeatId,
        source: ObjectId,
    ) -> Result<Self, StackWindowError> {
        let revision = self.next_revision(expected)?;
        if self.resolving().map(StackEntry::id) != Some(resolving) {
            return Err(StackWindowError::WrongResolution);
        }
        let changed = self.source_cost_change(actor, source, SourceCost::Tap, false)?;
        Ok(Self {
            revision,
            cards: changed.next,
            ..self.clone()
        })
    }
    /// Move the special action’s physical card to exile while retaining priority.
    pub(crate) fn exile_hand_special_action(
        &self,
        actor: SeatId,
        source: ObjectId,
    ) -> Result<(Self, ObjectMovement), StackWindowError> {
        let current = self.priority()?;
        let round = current.record_action(current.revision(), actor)?;
        let revision = self.next_revision(self.revision)?;
        let before = self
            .cards
            .get(source)
            .filter(|c| c.zone() == ObjectZone::Hand(actor) && c.card_id().is_some())
            .ok_or(StackWindowError::NotOwnHand)?
            .clone();
        let moved = self
            .cards
            .apply_zone_move(self.cards.revision(), source, ObjectDestination::Exile)?
            .require_moved()?;
        let next = Self {
            cards: moved.next,
            revision,
            status: Status::Priority(round),
            ..self.clone()
        };
        Ok((
            next,
            ObjectMovement {
                before,
                after: moved.after,
            },
        ))
    }
    /// Trusted ordinary land special action after program, timing and allowance
    /// validation by the enclosing owners. No stack identity is allocated.
    #[cfg(test)]
    pub(crate) fn record_land_play(
        &self,
        expected: u64,
        actor: SeatId,
        land: ObjectId,
    ) -> Result<(Self, crate::actions::LandPlayRecord), StackWindowError> {
        self.record_land_play_replaced(
            expected,
            actor,
            land,
            ObjectDestination::Battlefield { controller: actor },
            false,
        )
    }
    pub(crate) fn record_land_play_replaced(
        &self,
        expected: u64,
        actor: SeatId,
        land: ObjectId,
        destination: ObjectDestination,
        permitted_outside_hand: bool,
    ) -> Result<(Self, crate::actions::LandPlayRecord), StackWindowError> {
        let revision = self.next_revision(expected)?;
        let round = self.priority()?;
        let round = round.record_action(round.revision(), actor)?;
        let before = self
            .cards
            .get(land)
            .ok_or(ObjectIdentityError::UnknownObject)?;
        if before.zone() != ObjectZone::Hand(actor)
            && !(permitted_outside_hand
                && (before.zone() == ObjectZone::Exile
                    || before.zone() == ObjectZone::Library(actor)
                        && self.cards.library(actor).first() == Some(&land)))
        {
            return Err(StackWindowError::NotOwnHand);
        }
        if before.card_id().is_none() {
            return Err(ObjectIdentityError::NotCard.into());
        }
        let remains = matches!((before.zone(), destination), (ObjectZone::Hand(owner), ObjectDestination::Hand) if owner == before.owner())
            || matches!(
                (before.zone(), destination),
                (ObjectZone::Exile, ObjectDestination::Exile)
            );
        let (cards, after) = if remains {
            (self.cards.clone(), before.clone())
        } else {
            let changed = self
                .cards
                .apply_zone_move(self.cards.revision(), land, destination)?
                .require_moved()?;
            (changed.next, changed.after)
        };
        let record = crate::actions::LandPlayRecord {
            player: actor,
            before: before.clone(),
            after,
        };
        Ok((
            Self {
                revision,
                cards,
                status: Status::Priority(round),
                ..self.clone()
            },
            record,
        ))
    }
    #[cfg(test)]
    pub(crate) fn set_test_object_revision(&mut self, revision: u64) {
        self.cards.set_test_revision(revision);
    }
    #[cfg(test)]
    pub(crate) fn set_test_next_object_identity(&mut self, next_identity: u64) {
        self.cards.set_test_next_identity(next_identity);
    }
    #[cfg(test)]
    pub(crate) fn set_test_priority_revision(&mut self, revision: u64) {
        let Status::Priority(round) = &mut self.status else {
            panic!("priority fault injection requires a priority window");
        };
        round.set_test_revision(revision);
    }
    /// Physical cost only. The caller separately establishes ability legality
    /// and either ordinary priority action or trusted resolution payment authority.
    fn source_cost_change(
        &self,
        actor: SeatId,
        source: ObjectId,
        cost: SourceCost,
        madness: bool,
    ) -> Result<crate::object_identity::ObjectChange, StackWindowError> {
        let card = self
            .cards
            .get(source)
            .ok_or(ObjectIdentityError::UnknownObject)?;
        if cost.is_from_graveyard() {
            if card.zone() != ObjectZone::Graveyard(actor) {
                return Err(StackWindowError::WrongController);
            }
        } else if cost.is_ninjutsu() {
            if !cost.can_activate_ninjutsu(card, actor) {
                return Err(StackWindowError::NotOwnHand);
            }
        } else if cost.discards_source() {
            if card.zone() != ObjectZone::Hand(actor) {
                return Err(StackWindowError::NotOwnHand);
            }
        } else {
            if card.zone() != ObjectZone::Battlefield {
                return Err(ObjectIdentityError::NotPermanent.into());
            }
            if card.controller() != Some(actor)
                && !matches!(
                    cost,
                    SourceCost::Counter {
                        tap_source: false,
                        sacrifice_source: false,
                        ..
                    } | SourceCost::Exile {
                        tap_source: false,
                        from_graveyard: false,
                    }
                )
            {
                return Err(StackWindowError::WrongController);
            }
        }
        if matches!(
            cost,
            SourceCost::Power { .. }
                | SourceCost::SacrificeSelected { .. }
                | SourceCost::Discard { .. }
                | SourceCost::GraveyardDiscard { .. }
                | SourceCost::ExileGraveyard { .. }
                | SourceCost::GraveyardExileOther { .. }
                | SourceCost::Mill { .. }
                | SourceCost::PayLife {
                    sacrifice: Some(_),
                    ..
                }
        ) {
            return Err(StackWindowError::InconsistentStack);
        }
        if cost.sacrifices_source() || cost.exiles_source() || cost.discards_source() {
            let destination = if cost.exiles_source() || (madness && cost.discards_source()) {
                ObjectDestination::Exile
            } else {
                card.unearth_destination(ObjectDestination::Graveyard)
            };
            if cost.taps_source() {
                if card.tapped() != Some(false) {
                    return Err(StackWindowError::SourceTapped);
                }
                let tapped = self.cards.set_tapped(self.cards.revision(), source, true)?;
                return Ok(tapped
                    .next
                    .apply_zone_move(tapped.next.revision(), source, destination)?
                    .require_moved()?);
            }
            return Ok(self
                .cards
                .apply_zone_move(self.cards.revision(), source, destination)?
                .require_moved()?);
        }
        if cost.returns_source_to_hand() {
            if cost.taps_source() {
                if card.tapped() != Some(false) {
                    return Err(StackWindowError::SourceTapped);
                }
                let tapped = self.cards.set_tapped(self.cards.revision(), source, true)?;
                return Ok(tapped
                    .next
                    .apply_zone_move(tapped.next.revision(), source, ObjectDestination::Hand)?
                    .require_moved()?);
            }
            return Ok(self
                .cards
                .apply_zone_move(self.cards.revision(), source, ObjectDestination::Hand)?
                .require_moved()?);
        }
        if cost.untaps_source() {
            if card.tapped() != Some(true) {
                return Err(StackWindowError::SourceTapped);
            }
            return Ok(self
                .cards
                .set_tapped(self.cards.revision(), source, false)?);
        }
        if cost.taps_source() && !matches!(cost, SourceCost::TapPermanents { .. }) {
            if card.tapped() != Some(false) {
                return Err(StackWindowError::SourceTapped);
            }
            return Ok(self.cards.set_tapped(self.cards.revision(), source, true)?);
        }
        Ok(crate::object_identity::ObjectChange {
            next: self.cards.clone(),
            before: Some(card.clone()),
            after: card.clone(),
        })
    }
    /// Trusted native targeted activation with a source cost. No mana payment or
    /// general ability legality is inferred here. The source snapshot precedes cost.
    pub fn record_paid_targeted_activation(
        &self,
        expected: u64,
        actor: SeatId,
        source: ObjectId,
        cost: SourceCost,
        clauses: Vec<TargetClause>,
        choices: Vec<TargetChoice>,
    ) -> Result<PaidActivation, StackWindowError> {
        let before = self
            .cards
            .get(source)
            .ok_or(ObjectIdentityError::UnknownObject)?
            .clone();
        let mut next =
            self.record_targeted_activation(expected, actor, before.clone(), clauses, choices)?;
        let entry = next
            .stack
            .last()
            .ok_or(StackWindowError::InconsistentStack)?
            .id();
        let (paid, after) = next.pay_announced_source_cost(entry, actor, source, cost, false)?;
        next = paid;
        Ok(PaidActivation {
            state: next,
            entry,
            source_before: before,
            source_after: after,
        })
    }
    /// Physical cost continuation on the actual already-announced ability window.
    /// Its action/window revision was reserved by admission; only the registry advances here.
    pub(crate) fn pay_announced_source_cost(
        &self,
        entry: StackEntryId,
        actor: SeatId,
        source: ObjectId,
        cost: SourceCost,
        madness: bool,
    ) -> Result<(Self, GameObject), StackWindowError> {
        let admitted = self
            .stack
            .last()
            .ok_or(StackWindowError::InconsistentStack)?;
        if admitted.id() != entry
            || admitted.controller() != actor
            || !matches!(admitted.kind(), StackKind::ActivatedAbility { source: origin } if origin.id() == source)
        {
            return Err(StackWindowError::InconsistentStack);
        }
        let changed = self.source_cost_change(actor, source, cost, madness)?;
        let mut next = self.clone();
        next.cards = changed.next;
        Ok((next, changed.after))
    }
    pub(crate) fn pay_announced_source_to(
        &self,
        entry: StackEntryId,
        actor: SeatId,
        source: ObjectId,
        destination: ObjectDestination,
    ) -> Result<(Self, GameObject), StackWindowError> {
        let admitted = self
            .stack
            .last()
            .ok_or(StackWindowError::InconsistentStack)?;
        if admitted.id() != entry
            || admitted.controller() != actor
            || !matches!(admitted.kind(), StackKind::ActivatedAbility { source: origin } if origin.id() == source)
            || !matches!(
                destination,
                ObjectDestination::Graveyard | ObjectDestination::Exile | ObjectDestination::Hand
            )
        {
            return Err(StackWindowError::InconsistentStack);
        }
        let card = self
            .cards
            .get(source)
            .ok_or(ObjectIdentityError::UnknownObject)?;
        if !(card.zone() == ObjectZone::Battlefield
            && (card.controller() == Some(actor) || destination == ObjectDestination::Exile)
            || card.zone() == ObjectZone::Graveyard(actor))
        {
            return Err(StackWindowError::WrongController);
        }
        let changed = self
            .cards
            .apply_zone_move(self.cards.revision(), source, destination)?
            .require_moved()?;
        let mut next = self.clone();
        next.cards = changed.next;
        Ok((next, changed.after))
    }
    pub(crate) fn record_library_shuffle_moves(
        &self,
        entry: StackEntryId,
        player: SeatId,
        collective: bool,
        ability_source: Option<ObjectId>,
        moves: &[(ObjectId, ObjectDestination)],
    ) -> Result<(Self, Vec<ObjectMovement>), StackWindowError> {
        let targets_player = self
            .target_assessment()
            .and_then(|assessment| assessment.get(crate::TargetId(0)))
            .filter(|target| target.legality == crate::targets::TargetLegality::Legal)
            .is_some_and(|target| {
                target.choice.target == crate::targets::TargetRef::Player(player)
            });
        let current = self
            .resolving()
            .filter(|current| current.id() == entry)
            .ok_or(StackWindowError::WrongResolution)?;
        if let Some(source) = ability_source {
            let exact_source = match current.kind() {
                StackKind::ActivatedAbility { source } => Some(source.id()),
                StackKind::TriggeredAbility { source } => Some(source.id()),
                StackKind::CardSpell(card) => Some(*card),
                _ => None,
            };
            if exact_source != Some(source) || !matches!(moves, [(id, _)] if *id == source) {
                return Err(StackWindowError::InvalidResolutionMove);
            }
        } else if !collective && current.controller() != player && !targets_player {
            return Err(StackWindowError::WrongResolution);
        }
        let mut unique = BTreeSet::new();
        for (id, destination) in moves {
            let object = self
                .cards
                .get(*id)
                .ok_or(ObjectIdentityError::UnknownObject)?;
            let eligible = if ability_source.is_some() {
                ((object.zone() == ObjectZone::Battlefield
                    && self.cards.phased_out_under(*id).is_none())
                    || object.zone() == ObjectZone::Stack)
                    && object.owner() == player
            } else {
                object.card_id().is_some()
                    && if collective {
                        matches!(
                            object.zone(),
                            ObjectZone::Hand(_) | ObjectZone::Graveyard(_) | ObjectZone::Exile
                        ) || (object.zone() == ObjectZone::Battlefield
                            && self.cards.phased_out_under(*id).is_none())
                    } else {
                        object.zone() == ObjectZone::Graveyard(player)
                    }
            };
            if !unique.insert(*id)
                || !eligible
                || !matches!(
                    destination,
                    ObjectDestination::Command
                        | ObjectDestination::LibraryTop
                        | ObjectDestination::Hand
                        | ObjectDestination::Exile
                        | ObjectDestination::Graveyard
                )
            {
                return Err(StackWindowError::InvalidResolutionMove);
            }
        }
        let actual = moves
            .iter()
            .copied()
            .filter(|(id, destination)| {
                self.cards.get(*id).is_some_and(|object| {
                    !matches!(
                        (object.zone(), destination),
                        (ObjectZone::Graveyard(_), ObjectDestination::Graveyard)
                            | (ObjectZone::Hand(_), ObjectDestination::Hand)
                    )
                })
            })
            .collect::<Vec<_>>();
        if actual.is_empty() {
            return Ok((self.clone(), Vec::new()));
        }
        self.apply_recorded_zone_moves(self.next_revision(self.revision)?, &actual)
    }
    pub(crate) fn record_graveyard_exile_moves(
        &self,
        entry: StackEntryId,
        moves: &[(ObjectId, ObjectDestination)],
    ) -> Result<(Self, Vec<ObjectMovement>), StackWindowError> {
        if self.resolving().is_none_or(|current| current.id() != entry) {
            return Err(StackWindowError::WrongResolution);
        }
        let mut unique = BTreeSet::new();
        for (id, destination) in moves {
            let card = self
                .cards
                .get(*id)
                .ok_or(ObjectIdentityError::UnknownObject)?;
            if !unique.insert(*id)
                || card.card_id().is_none()
                || !matches!(card.zone(), ObjectZone::Graveyard(_))
                || !matches!(
                    destination,
                    ObjectDestination::Hand
                        | ObjectDestination::Exile
                        | ObjectDestination::Graveyard
                )
            {
                return Err(StackWindowError::InvalidResolutionMove);
            }
        }
        let actual = moves
            .iter()
            .copied()
            .filter(|(_, destination)| *destination != ObjectDestination::Graveyard)
            .collect::<Vec<_>>();
        if actual.is_empty() {
            return Ok((self.clone(), Vec::new()));
        }
        self.apply_recorded_zone_moves(self.next_revision(self.revision)?, &actual)
    }
    /// Commit validated selected activation-cost departures as one movement cut.
    pub(crate) fn pay_selected_activation_costs(
        &self,
        entry: StackEntryId,
        actor: SeatId,
        action: crate::program::ZoneActionKind,
        moves: &[(ObjectId, ObjectDestination)],
    ) -> Result<(Self, Vec<ObjectMovement>), StackWindowError> {
        let admitted = self
            .stack
            .last()
            .ok_or(StackWindowError::InconsistentStack)?;
        if admitted.id() != entry
            || admitted.controller() != actor
            || !matches!(
                admitted.kind(),
                StackKind::ActivatedAbility { .. } | StackKind::Emblem { .. }
            )
            || moves.is_empty()
        {
            return Err(StackWindowError::InconsistentStack);
        }
        let mut unique = BTreeSet::new();
        for (id, destination) in moves {
            let object = self
                .cards
                .get(*id)
                .ok_or(ObjectIdentityError::UnknownObject)?;
            if !unique.insert(*id)
                || !matches!(
                    destination,
                    ObjectDestination::Hand
                        | ObjectDestination::Graveyard
                        | ObjectDestination::Exile
                )
                || !match action {
                    crate::program::ZoneActionKind::MillCost => {
                        moves.len() == 1 && self.cards.library(actor).first() == Some(id)
                    }
                    crate::program::ZoneActionKind::DiscardCost => {
                        object.zone() == ObjectZone::Hand(actor) && object.card_id().is_some()
                    }
                    crate::program::ZoneActionKind::ExileCost => {
                        object.zone() == ObjectZone::Graveyard(actor) && object.card_id().is_some()
                    }
                    crate::program::ZoneActionKind::SacrificeCost => {
                        object.zone() == ObjectZone::Battlefield
                            && object.controller() == Some(actor)
                    }
                    _ => false,
                }
            {
                return Err(StackWindowError::InvalidResolutionMove);
            }
        }
        let actual = moves
            .iter()
            .copied()
            .filter(|(id, destination)| {
                !matches!(
                    (self.cards.get(*id).map(GameObject::zone), destination),
                    (Some(ObjectZone::Hand(_)), ObjectDestination::Hand)
                        | (Some(ObjectZone::Graveyard(_)), ObjectDestination::Graveyard)
                )
            })
            .collect::<Vec<_>>();
        if actual.is_empty() {
            return Ok((self.clone(), Vec::new()));
        }
        self.apply_recorded_zone_moves(self.next_revision(self.revision)?, &actual)
    }
    /// Record an otherwise legal card cast with a new stack incarnation. This narrow
    /// move/push/reset is atomic in the returned native successor; it does not pay
    /// costs or implement the intervening casting procedure (CR 601).
    pub fn record_card_spell(
        &self,
        expected: u64,
        actor: SeatId,
        card: ObjectId,
    ) -> Result<Self, StackWindowError> {
        self.admit_card_spell(expected, actor, card, None)
    }
    pub(crate) fn admit_card_spell(
        &self,
        expected: u64,
        actor: SeatId,
        card: ObjectId,
        resolving: Option<StackEntryId>,
    ) -> Result<Self, StackWindowError> {
        let revision = self.next_revision(expected)?;
        let round = if let Some(entry) = resolving {
            if self.resolving().map(StackEntry::id) != Some(entry) {
                return Err(StackWindowError::WrongResolution);
            }
            None
        } else {
            let round = self.priority()?;
            Some(round.record_action(round.revision(), actor)?)
        };
        if self.cards.get(card).is_some_and(GameObject::is_token) {
            return Err(ObjectIdentityError::NotCard.into());
        }
        let moved = self
            .cards
            .apply_zone_move(
                self.cards.revision(),
                card,
                ObjectDestination::Stack { controller: actor },
            )?
            .require_moved()?;
        let mut next = self.clone();
        let mut entry = next.allocate(actor, StackKind::CardSpell(moved.after.id()))?;
        entry.caster = Some(actor);
        entry.cast_original_object = Some(card);
        entry.cast_from_command = moved
            .before
            .as_ref()
            .is_some_and(|c| c.zone() == ObjectZone::Command);
        entry.cast_from_graveyard = matches!(
            moved.before.as_ref().map(GameObject::zone),
            Some(ObjectZone::Graveyard(_))
        );
        if let Some(ObjectZone::Library(owner)) = moved.before.as_ref().map(GameObject::zone) {
            entry.cast_from_library = Some(owner);
        }
        if let Some(ObjectZone::Hand(player)) = moved.before.as_ref().map(GameObject::zone) {
            entry.cast_from_hand = Some(player);
        }
        entry.cast_summary.cast_from_graveyard = entry.cast_from_graveyard;
        entry.cast_summary.cast_from_exile = moved
            .before
            .as_ref()
            .is_some_and(|c| c.zone() == ObjectZone::Exile);
        entry.cast_summary.cast_from_outside_hand = entry.cast_from_hand != Some(actor);

        next.stack.push(entry);
        next.cards = moved.next;
        if let Some(round) = round {
            next.status = Status::Priority(round);
        }
        next.revision = revision;
        Ok(next)
    }
    /// Admit a castable spell copy (such as a prepared permanent's spell) directly onto the stack.
    pub(crate) fn admit_prepared_spell_copy(
        &self,
        expected: u64,
        actor: SeatId,
        definition: crate::program::Program,
    ) -> Result<Self, StackWindowError> {
        let revision = self.next_revision(expected)?;
        let round = self.priority()?;
        let round = round.record_action(round.revision(), actor)?;
        let mut next = self.clone();
        let copy_id = next.cards.insert_spell_copy(actor, definition)?;
        let mut entry = next.allocate(actor, StackKind::CardSpell(copy_id))?;
        entry.caster = Some(actor);
        entry.cast_original_object = Some(copy_id);
        next.stack.push(entry);
        next.status = Status::Priority(round);
        next.revision = revision;
        Ok(next)
    }
    /// Reverse an unfinished child spell announcement without undoing later
    /// mana-ability taps or damage. Only the original exile incarnation may return.
    pub(crate) fn reverse_child_cast(
        &self,
        parent: StackEntryId,
        child: StackEntryId,
        before: &GameObject,
        graveyard_below: &[ObjectId],
    ) -> Result<Self, StackWindowError> {
        if self.resolving().map(StackEntry::id) != Some(parent) {
            return Err(StackWindowError::WrongResolution);
        }
        let Some(entry) = self.stack.last().filter(|entry| entry.id() == child) else {
            return Err(StackWindowError::InconsistentStack);
        };
        let StackKind::CardSpell(stack_object) = entry.kind() else {
            return Err(StackWindowError::InconsistentStack);
        };
        let cards = self
            .cards
            .reverse_cast_announcement(before, *stack_object, graveyard_below)?;
        let mut next = self.clone();
        next.revision = self.next_revision(self.revision)?;
        next.stack.pop();
        next.cards = cards;
        Ok(next)
    }
    pub(crate) fn tap_for_cast_cost(
        &self,
        entry: StackEntryId,
        objects: &[ObjectId],
    ) -> Result<(Self, Vec<ObjectStatusChange>), StackWindowError> {
        let actor = self
            .stack
            .last()
            .filter(|value| value.id() == entry && matches!(value.kind(), StackKind::CardSpell(_)))
            .ok_or(StackWindowError::InconsistentStack)?
            .controller();
        if objects.iter().any(|id| {
            self.cards.get(*id).is_none_or(|object| {
                object.zone() != ObjectZone::Battlefield
                    || object.controller() != Some(actor)
                    || object.tapped() != Some(false)
            })
        }) {
            return Err(StackWindowError::InvalidResolutionMove);
        }
        let revision = self.next_revision(self.revision)?;
        let (cards, changes) = self
            .cards
            .set_tapped_batch(self.cards.revision(), objects, true)?;
        Ok((self.with_cards(revision, cards), changes))
    }
    /// Pay the prevalidated simultaneous creature taps of an announced crew/saddle activation.
    pub(crate) fn pay_power_contributors(
        &self,
        entry: StackEntryId,
        objects: &[ObjectId],
    ) -> Result<(Self, Vec<crate::object_identity::ObjectStatusChange>), StackWindowError> {
        let mut next = self.clone();
        let ability = next
            .stack
            .last_mut()
            .filter(|ability| {
                ability.id() == entry
                    && matches!(ability.kind(), StackKind::ActivatedAbility { .. })
            })
            .ok_or(StackWindowError::InconsistentStack)?;
        let (cards, changes) = self
            .cards
            .set_tapped_batch(self.cards.revision(), objects, true)?;
        ability.power_contributors = objects.to_vec();
        next.cards = cards;
        Ok((next, changes))
    }

    pub(crate) fn set_orientation(
        &mut self,
        object: ObjectId,
        kind: Option<crate::face_down::FaceDownKind>,
    ) -> Result<(), StackWindowError> {
        self.cards.set_orientation(object, kind)?;
        Ok(())
    }
    pub(crate) fn record_turn_face_up(
        &self,
        actor: SeatId,
        object: ObjectId,
    ) -> Result<Self, StackWindowError> {
        let current = self.priority()?;
        let round = current.record_action(current.revision(), actor)?;
        let mut next = self.clone();
        next.revision = self.next_revision(self.revision)?;
        next.set_orientation(object, None)?;
        next.status = Status::Priority(round);
        Ok(next)
    }

    /// Apply one already validated object payment to the announced spell.
    pub(crate) fn pay_cast_resource(
        &self,
        entry: StackEntryId,
        resource: crate::mana::CastResource,
        madness: bool,
    ) -> Result<(Self, GameObject, GameObject), StackWindowError> {
        self.pay_cast_resource_replaced(entry, resource, madness, None)
    }
    pub(crate) fn pay_cast_resource_replaced(
        &self,
        entry: StackEntryId,
        resource: crate::mana::CastResource,
        madness: bool,
        replacement: Option<ObjectDestination>,
    ) -> Result<(Self, GameObject, GameObject), StackWindowError> {
        if self.stack.last().is_none_or(|top| top.id != entry) {
            return Err(StackWindowError::InconsistentStack);
        }
        let revision = self.next_revision(self.revision)?;
        let id = resource.object();
        let before = self
            .cards
            .get(id)
            .ok_or(ObjectIdentityError::UnknownObject)?
            .clone();
        let replaced = replacement.is_some();
        let destination = replacement.or(match resource {
            crate::mana::CastResource::WebSlinging(_) | crate::mana::CastResource::Sneak(_) => {
                Some(ObjectDestination::Hand)
            }
            crate::mana::CastResource::ExileHandForAlternative(_)
            | crate::mana::CastResource::ExileCreatureForReduction(_)
            | crate::mana::CastResource::ExileGraveyardCreatureForSpell(_)
            | crate::mana::CastResource::ExileGraveyardCardForSpell(_)
            | crate::mana::CastResource::Escape(_)
            | crate::mana::CastResource::Delve(_) => Some(ObjectDestination::Exile),
            crate::mana::CastResource::Emerge(_)
            | crate::mana::CastResource::Bargain(_)
            | crate::mana::CastResource::SacrificeForSpell(_)
            | crate::mana::CastResource::FlashbackSacrifice(_)
            | crate::mana::CastResource::Retrace(_)
            | crate::mana::CastResource::DiscardForSpell(_)
            | crate::mana::CastResource::JumpStart(_) => Some(if madness {
                ObjectDestination::Exile
            } else {
                ObjectDestination::Graveyard
            }),
            _ => None,
        });
        let (cards, after) = if let Some(destination) = destination {
            let moved = self
                .cards
                .apply_zone_move(
                    self.cards.revision(),
                    id,
                    if replaced {
                        destination
                    } else {
                        before.unearth_destination(destination)
                    },
                )?
                .require_moved()?;
            (moved.next, moved.after)
        } else {
            let changed = self.cards.set_tapped(self.cards.revision(), id, true)?;
            (changed.next, changed.after)
        };
        Ok((self.with_cards(revision, cards), before, after))
    }

    pub(crate) fn note_attacking_defender(
        &mut self,
        id: StackEntryId,
        defender: SeatId,
    ) -> Result<(), StackWindowError> {
        let entry = self
            .stack
            .last_mut()
            .filter(|s| s.id == id)
            .ok_or(StackWindowError::InconsistentStack)?;
        entry.attacking_defender = Some(defender);
        Ok(())
    }
    pub(crate) fn set_day_faces(
        &mut self,
        clock: crate::day_night::DayNight,
        changed: &[ObjectId],
    ) -> Result<(), StackWindowError> {
        self.cards.set_day_faces(clock, changed)?;
        Ok(())
    }
    pub(crate) fn transform(&mut self, source: ObjectId) -> Result<(), StackWindowError> {
        self.cards.transform(source)?;
        Ok(())
    }
    /// Install an entry replacement's controller inside the unobserved zone-change cut.
    pub(crate) fn set_entering_controller(
        &mut self,
        object: ObjectId,
        player: SeatId,
    ) -> Result<(), StackWindowError> {
        self.validate_player(player)?;
        if self
            .cards
            .get(object)
            .is_none_or(|card| card.zone() != ObjectZone::Battlefield)
        {
            return Err(StackWindowError::InvalidResolutionMove);
        }
        self.cards = self
            .cards
            .apply_controller(self.cards.revision(), object, player)?
            .next;
        Ok(())
    }
    pub(crate) fn enter_tapped(&mut self, id: ObjectId) -> Result<GameObject, StackWindowError> {
        if let Some(object) = self
            .cards
            .get(id)
            .filter(|object| object.tapped() == Some(true))
        {
            return Ok(object.clone());
        }
        let change = self.cards.set_tapped(self.cards.revision(), id, true)?;
        self.cards = change.next;
        Ok(change.after)
    }
    pub(crate) fn mark_protected_by_spent_mana(
        &mut self,
        id: StackEntryId,
    ) -> Result<(), StackWindowError> {
        let entry = self
            .stack
            .last_mut()
            .filter(|entry| entry.id == id && matches!(entry.kind, StackKind::CardSpell(_)))
            .ok_or(StackWindowError::InconsistentStack)?;
        entry.protected_by_spent_mana = true;
        Ok(())
    }
    pub(crate) fn mark_main_phase_cast(
        &mut self,
        id: StackEntryId,
    ) -> Result<(), StackWindowError> {
        let entry = self
            .stack
            .last_mut()
            .filter(|entry| entry.id == id)
            .ok_or(StackWindowError::InconsistentStack)?;
        entry.cast_during_own_main = true;
        entry.cast_summary.cast_during_own_main = true;
        Ok(())
    }
    pub(crate) fn record_cast_mana_summary(
        &mut self,
        id: StackEntryId,
        spend: &[crate::mana::ManaSpend],
        snow: RulesInteger,
        treasure: RulesInteger,
        creature: RulesInteger,
    ) -> Result<(), StackWindowError> {
        if let Some(entry) = self.stack.iter_mut().find(|entry| entry.id == id) {
            entry
                .cast_summary
                .record_spend(spend, snow, treasure, creature);
        }
        Ok(())
    }
    pub(crate) fn mark_conditional_cost(
        &mut self,
        id: StackEntryId,
    ) -> Result<(), StackWindowError> {
        let entry = self
            .stack
            .last_mut()
            .filter(|s| s.id == id)
            .ok_or(StackWindowError::InconsistentStack)?;
        entry.conditional_cost = true;
        entry.cast_summary.paid_alternative = true;
        Ok(())
    }
    pub(crate) fn set_chosen_x(
        &mut self,
        id: StackEntryId,
        x: RulesInteger,
    ) -> Result<(), StackWindowError> {
        self.stack
            .last_mut()
            .filter(|entry| entry.id == id)
            .ok_or(StackWindowError::InconsistentStack)?
            .chosen_x = Some(x);
        Ok(())
    }
    /// Bind X on the currently resolving stack entry (for `Instruction::BindResolutionX`).
    pub(crate) fn set_resolving_chosen_x(
        &mut self,
        x: RulesInteger,
    ) -> Result<(), StackWindowError> {
        let Status::Resolving { entry, .. } = &mut self.status else {
            return Err(StackWindowError::InconsistentStack);
        };
        entry.chosen_x = Some(x);
        Ok(())
    }
    pub(crate) fn mark_sunburst_colors(
        &mut self,
        entry: StackEntryId,
        spend: &[crate::mana::ManaSpend],
    ) -> Result<(), StackWindowError> {
        let colors = spend
            .iter()
            .filter(|s| s.mana_type != crate::mana::ManaType::Colourless && s.amount.is_positive())
            .map(|s| s.mana_type)
            .collect::<BTreeSet<_>>();
        self.stack
            .last_mut()
            .filter(|s| s.id == entry)
            .ok_or(StackWindowError::InconsistentStack)?
            .sunburst_colors =
            u8::try_from(colors.len()).map_err(|_| StackWindowError::InconsistentStack)?;
        Ok(())
    }
    pub(crate) fn set_replicate(
        &mut self,
        id: StackEntryId,
        paid: BTreeMap<usize, RulesInteger>,
        grants: BTreeMap<crate::characteristics::EffectInstanceKey, usize>,
    ) -> Result<(), StackWindowError> {
        let entry = self
            .stack
            .last_mut()
            .filter(|s| s.id == id)
            .ok_or(StackWindowError::InconsistentStack)?;
        entry.replicated = paid;
        entry.granted_replicate = grants;
        Ok(())
    }
    pub(crate) fn set_additional_costs(
        &mut self,
        id: StackEntryId,
        buyback: bool,
        bargained: bool,
        kicked: BTreeMap<usize, RulesInteger>,
    ) -> Result<(), StackWindowError> {
        let entry = self
            .stack
            .last_mut()
            .filter(|s| s.id == id)
            .ok_or(StackWindowError::InconsistentStack)?;
        entry.bargained = bargained;
        entry.cast_summary.bargained = bargained;
        if buyback || bargained || !kicked.is_empty() {
            entry.cast_summary.additional_cost_paid = true;
        }
        entry.buyback = buyback;
        entry.kicked = kicked;
        Ok(())
    }
    pub(crate) fn mark_additional_cost_paid(
        &mut self,
        id: StackEntryId,
    ) -> Result<(), StackWindowError> {
        let entry = self
            .stack
            .last_mut()
            .filter(|s| s.id == id)
            .ok_or(StackWindowError::InconsistentStack)?;
        entry.cast_summary.additional_cost_paid = true;
        Ok(())
    }
    /// Mark the freshly announced physical spell. Copies allocate their own entry.
    pub(crate) fn mark_exile_cast(
        &mut self,
        id: StackEntryId,
        method: ExileCast,
    ) -> Result<(), StackWindowError> {
        let entry = self
            .stack
            .last_mut()
            .filter(|entry| entry.id == id && matches!(entry.kind, StackKind::CardSpell(_)))
            .ok_or(StackWindowError::InconsistentStack)?;
        entry.exile_cast = Some(method);
        Ok(())
    }

    /// Trusted batch from the actual resolving instruction, with participant creators.
    /// This creates no stack entries, casting actions, checks or priority.
    /// The executor adopts timestamps and one publication atomically.
    pub(crate) fn record_token_creations(
        &self,
        expected: u64,
        tokens: Vec<(SeatId, crate::program::Program)>,
        vocabulary: &spellwake_contracts::creature_types::CreatureTypeVocabulary,
    ) -> Result<(Self, Vec<GameObject>), StackWindowError> {
        if expected != self.revision {
            return Err(StackWindowError::StaleRevision);
        }
        self.resolving().ok_or(StackWindowError::NotResolving)?;
        for (creator, _) in &tokens {
            self.validate_player(*creator)?;
        }
        if tokens.is_empty() {
            return Ok((self.clone(), vec![]));
        }
        let revision = self.next_revision(expected)?;
        let created =
            self.cards
                .insert_materialized_tokens(self.cards.revision(), tokens, vocabulary)?;
        Ok((self.with_cards(revision, created.next), created.objects))
    }
    /// Trusted resolution-only creation of a nonpermanent spell copy. This is not
    /// casting, pays no costs and grants no priority. The executor snapshots the
    /// matching payload and validates additional changed-target predicates.
    pub fn record_spell_copy(
        &self,
        expected: u64,
        original: StackEntryId,
        controller: SeatId,
        changes: Vec<TargetChoice>,
    ) -> Result<Self, StackWindowError> {
        self.next_revision(expected)?;
        if !matches!(self.status, Status::Resolving { .. }) {
            return Err(StackWindowError::NotResolving);
        }
        self.validate_player(controller)?;
        let source = self
            .stack
            .iter()
            .find(|entry| entry.id == original)
            .ok_or(StackWindowError::NotSpell)?;
        self.copy_known_spell(expected, source, controller, changes)
    }
    pub(crate) fn copy_known_ability(
        &self,
        source: &StackEntry,
        controller: SeatId,
        changes: Vec<TargetChoice>,
    ) -> Result<Self, StackWindowError> {
        let revision = self.next_revision(self.revision)?;
        if !matches!(self.status, Status::Resolving { .. }) {
            return Err(StackWindowError::NotResolving);
        }
        if matches!(
            source.kind,
            StackKind::CardSpell(_) | StackKind::SpellCopy { .. }
        ) {
            return Err(StackWindowError::InconsistentStack);
        }
        self.validate_player(controller)?;
        let mut next = self.clone();
        next.revision = revision;
        let id = next.allocate(controller, source.kind.clone())?.id;
        let mut copy = source.clone();
        copy.controller = controller;
        copy.id = id;
        copy.ability_copy_of = Some(source.id);
        next.stack.push(copy.clone());
        copy.targets = source.targets.copy_with_changes(&next, id, changes)?;
        *next
            .stack
            .last_mut()
            .ok_or(StackWindowError::InconsistentStack)? = copy;
        Ok(next)
    }
    pub(crate) fn copy_known_spell(
        &self,
        expected: u64,
        source: &StackEntry,
        controller: SeatId,
        changes: Vec<TargetChoice>,
    ) -> Result<Self, StackWindowError> {
        let revision = self.next_revision(expected)?;
        if !matches!(self.status, Status::Resolving { .. }) {
            return Err(StackWindowError::NotResolving);
        }
        self.validate_player(controller)?;
        let original = source.id();
        if !matches!(
            source.kind,
            StackKind::CardSpell(_) | StackKind::SpellCopy { .. }
        ) {
            return Err(StackWindowError::NotSpell);
        }
        let targets = source.targets.clone();
        let buyback = source.buyback;
        let kicked = source.kicked.clone();
        let mut next = self.clone();
        let mut entry = next.allocate(
            controller,
            StackKind::SpellCopy {
                copied_from: original,
                owner: controller,
            },
        )?;
        next.stack.push(entry.clone());
        entry.chosen_x.clone_from(&source.chosen_x);
        entry.attacking_defender = source.attacking_defender;
        entry.conditional_cost = source.conditional_cost;
        entry.bargained = source.bargained;
        entry.cast_summary.bargained = source.cast_summary.bargained;
        entry.cast_summary.gift_promised = source.cast_summary.gift_promised;
        entry.cast_summary.cleaved = source.cast_summary.cleaved;
        entry.cast_summary.evidence_collected = source.cast_summary.evidence_collected;
        entry.cast_summary.paid_alternative = source.cast_summary.paid_alternative;
        entry.cast_summary.additional_cost_paid = source.cast_summary.additional_cost_paid;
        entry.cast_summary.squad_count = source.cast_summary.squad_count;
        entry.buyback = buyback;
        entry.kicked = kicked;
        entry.replicated = source.replicated.clone();
        entry.targets = targets.copy_with_changes(&next, entry.id, changes)?;
        *next
            .stack
            .last_mut()
            .ok_or(StackWindowError::InconsistentStack)? = entry.clone();
        next.copies.insert(
            entry.id,
            SpellCopyObject {
                permanent_object: None,
                id: entry.id,
                owner: controller,
                controller: Some(controller),
                zone: ObjectZone::Stack,
            },
        );
        next.revision = revision;
        Ok(next)
    }
    pub(crate) fn give_copy_permanent_definition(
        &mut self,
        entry: StackEntryId,
        definition: crate::program::Program,
    ) -> Result<(), StackWindowError> {
        let copy = self
            .copies
            .get_mut(&entry)
            .ok_or(StackWindowError::InconsistentStack)?;
        if copy.permanent_object.is_some() || copy.zone != ObjectZone::Stack {
            return Err(StackWindowError::InconsistentStack);
        }
        copy.permanent_object = Some(
            self.cards
                .insert_permanent_spell_copy(copy.owner, definition)?,
        );
        Ok(())
    }
    pub(crate) fn spell_object(&self, entry: &StackEntry) -> Option<ObjectId> {
        match entry.kind() {
            StackKind::CardSpell(object) => Some(*object),
            StackKind::SpellCopy { .. } => self.copies.get(&entry.id())?.permanent_object,
            _ => None,
        }
    }
    /// CR 112.2. A change on the stack leaves the caster, owner and target choices intact.
    pub(crate) fn gain_spell_control(
        &mut self,
        id: StackEntryId,
        controller: SeatId,
    ) -> Result<SeatId, StackWindowError> {
        self.validate_player(controller)?;
        let index = self
            .stack
            .iter()
            .position(|entry| entry.id == id)
            .ok_or(StackWindowError::InconsistentStack)?;
        let previous = self.stack[index].controller;
        if previous == controller {
            return Ok(previous);
        }
        self.set_spell_controller(index, controller)?;
        self.stack[index].previous_controllers.push(previous);
        Ok(previous)
    }

    pub(crate) fn end_departed_spell_control(
        &mut self,
        players: &BTreeSet<SeatId>,
    ) -> Result<Vec<(StackEntryId, SeatId, SeatId)>, StackWindowError> {
        let mut changes = Vec::new();
        for index in 0..self.stack.len() {
            let entry = &mut self.stack[index];
            entry
                .previous_controllers
                .retain(|seat| !players.contains(seat));
            if !players.contains(&entry.controller())
                || matches!(entry.kind(), StackKind::CardSpell(id) if self.cards.get(*id).is_none())
                || matches!(entry.kind(), StackKind::SpellCopy { owner, .. } if players.contains(owner))
            {
                continue;
            }
            if let Some(restored) = entry.previous_controllers.pop() {
                let (id, departed) = (entry.id(), entry.controller());
                self.set_spell_controller(index, restored)?;
                changes.push((id, departed, restored));
            }
        }
        Ok(changes)
    }

    fn set_spell_controller(
        &mut self,
        index: usize,
        controller: SeatId,
    ) -> Result<(), StackWindowError> {
        let revision = self.next_revision(self.revision)?;
        match self.stack[index].kind {
            StackKind::CardSpell(object) => {
                self.cards = self
                    .cards
                    .apply_controller(self.cards.revision(), object, controller)?
                    .next;
            }
            StackKind::SpellCopy { .. } => {
                let copy = self
                    .copies
                    .get_mut(&self.stack[index].id)
                    .ok_or(StackWindowError::InconsistentStack)?;
                if let Some(object) = copy.permanent_object {
                    self.cards = self
                        .cards
                        .apply_controller(self.cards.revision(), object, controller)?
                        .next;
                }
                copy.controller = Some(controller);
            }
            _ => return Err(StackWindowError::InvalidResolutionMove),
        }
        self.stack[index].controller = controller;
        self.revision = revision;
        Ok(())
    }
    /// Replace only the selected occurrences, leaving untouched targets and stack order intact.
    pub(crate) fn change_targets(
        &mut self,
        id: StackEntryId,
        changes: Vec<TargetChoice>,
    ) -> Result<(), StackWindowError> {
        let resolving = self.resolving().ok_or(StackWindowError::NotResolving)?;
        if resolving.id() == id {
            return Err(StackWindowError::InvalidResolutionMove);
        }
        let index = self
            .stack
            .iter()
            .position(|entry| entry.id == id)
            .ok_or(StackWindowError::InconsistentStack)?;
        let targets = self.stack[index].targets.retarget(self, id, changes)?;
        let revision = self.next_revision(self.revision)?;
        self.stack[index].targets = targets;
        self.revision = revision;
        Ok(())
    }
    /// CR 704.5e: exited spell copies cease to exist during pending state checks,
    /// not as an instruction in the middle of resolution.
    pub fn cease_departed_copies(&self, expected: u64) -> Result<Self, StackWindowError> {
        let revision = self.next_revision(expected)?;
        self.require_checks()?;
        let mut next = self.clone();
        for copy in self
            .copies
            .values()
            .filter(|copy| copy.zone != ObjectZone::Stack)
        {
            if let Some(object) = copy.permanent_object {
                next.cards.cease_departed_permanent_copy(object)?;
            }
        }
        next.copies.retain(|_, copy| copy.zone == ObjectZone::Stack);
        next.revision = revision;
        Ok(next)
    }
    /// Cease only the exact frozen copy cohort from the actual check pass.
    pub(crate) fn cease_check_copies(
        &self,
        expected: u64,
        cohort: &BTreeMap<StackEntryId, SpellCopyObject>,
    ) -> Result<Self, StackWindowError> {
        let actual = self
            .copies
            .iter()
            .filter(|(_, copy)| copy.zone != ObjectZone::Stack)
            .map(|(id, copy)| (*id, copy.clone()))
            .collect::<BTreeMap<_, _>>();
        if &actual != cohort {
            return Err(StackWindowError::InvalidCheckMove);
        }
        self.cease_departed_copies(expected)
    }
    /// Apply only the exact already-departed token cohort from one frozen check.
    /// Newly dying tokens are outside this cohort and remain until the next check.
    pub(crate) fn cease_tokens(
        &self,
        expected: u64,
        objects: &BTreeSet<ObjectId>,
    ) -> Result<(Self, Vec<GameObject>), StackWindowError> {
        let revision = self.next_revision(expected)?;
        self.require_checks()?;
        let mut cards = self.cards.clone();
        let mut ceased = Vec::with_capacity(objects.len());
        for id in objects {
            let removal = cards.cease_token(cards.revision(), *id)?;
            cards = removal.next;
            ceased.push(removal.object);
        }
        Ok((self.with_cards(revision, cards), ceased))
    }
    fn copy_exit(
        &self,
        next: &Self,
        before: &SpellCopyObject,
        after: &SpellCopyObject,
    ) -> Result<StackExit, StackWindowError> {
        if let (Some(old), Some(new)) = (before.permanent_object, after.permanent_object) {
            return Ok(StackExit::Object(ObjectMovement {
                before: self
                    .cards
                    .get(old)
                    .cloned()
                    .ok_or(StackWindowError::InconsistentStack)?,
                after: next
                    .cards
                    .get(new)
                    .cloned()
                    .ok_or(StackWindowError::InconsistentStack)?,
            }));
        }
        Ok(StackExit::Copy {
            before: before.clone(),
            after: after.clone(),
        })
    }
    fn move_copy(
        &self,
        revision: u64,
        id: StackEntryId,
        destination: ObjectDestination,
    ) -> Result<Self, StackWindowError> {
        let returning = destination == ObjectDestination::Hand
            && self.resolving().is_some_and(|s| s.id == id && s.buyback);
        if self
            .copies
            .get(&id)
            .is_none_or(|copy| copy.permanent_object.is_none())
            && !matches!(
                destination,
                ObjectDestination::Graveyard | ObjectDestination::Exile
            )
            && !returning
        {
            return Err(StackWindowError::UnsupportedTargetOperation);
        }
        let mut next = self.clone();
        let copy = next
            .copies
            .get_mut(&id)
            .ok_or(StackWindowError::InconsistentStack)?;
        if copy.zone != ObjectZone::Stack {
            return Err(StackWindowError::InconsistentStack);
        }
        if let Some(object) = copy.permanent_object {
            let moved = next
                .cards
                .apply_zone_move(next.cards.revision(), object, destination)?
                .require_moved()?;
            copy.zone = moved.after.zone();
            copy.controller = moved.after.controller();
            copy.permanent_object = Some(moved.after.id());
            next.cards = moved.next;
        } else {
            copy.zone = if destination == ObjectDestination::Exile {
                ObjectZone::Exile
            } else if returning {
                ObjectZone::Hand(copy.owner)
            } else {
                ObjectZone::Graveyard(copy.owner)
            };
            copy.controller = None;
        }
        next.stack.retain(|entry| entry.id != id);
        next.revision = revision;
        Ok(next)
    }
    /// Record an otherwise legal nonmana activation. The supplied source snapshot
    /// can precede costs that moved the source; it need not still be current.
    /// This raw continuation executes no costs. The scoped paid wrapper handles
    /// Tap/Sacrifice; full ability/source context and general costs remain absent.
    pub fn record_activation(
        &self,
        expected: u64,
        actor: SeatId,
        source: GameObject,
    ) -> Result<Self, StackWindowError> {
        let revision = self.next_revision(expected)?;
        let round = self.priority()?;
        let round = round.record_action(round.revision(), actor)?;
        self.validate_source(&source)?;
        let mut next = self.clone();
        let entry = next.allocate(actor, StackKind::ActivatedAbility { source })?;
        next.stack.push(entry);
        next.status = Status::Priority(round);
        next.revision = revision;
        Ok(next)
    }
    pub(crate) fn record_emblem_activation(
        &self,
        expected: u64,
        actor: SeatId,
        emblem: crate::emblem::EmblemId,
    ) -> Result<(Self, StackEntryId), StackWindowError> {
        let revision = self.next_revision(expected)?;
        let round = self.priority()?;
        let round = round.record_action(round.revision(), actor)?;
        self.validate_player(actor)?;
        let mut next = self.clone();
        let entry = next.allocate(actor, StackKind::Emblem { emblem })?;
        let id = entry.id();
        next.stack.push(entry);
        next.status = Status::Priority(round);
        next.revision = revision;
        Ok((next, id))
    }
    /// Apply a preordered nonmana trigger batch during checks. The same candidate remains
    /// pending; the changed stack requires a fresh pass round. No discovery or player
    /// ordering decision is synthesized, and no controller silently gains priority.
    pub fn record_ordered_triggers(
        &self,
        expected: u64,
        triggers: Vec<OrderedTrigger>,
    ) -> Result<Self, StackWindowError> {
        // Explicitly receipt-dropping trusted convenience. Executor placement
        // consumes the allocation receipt instead of guessing the new entries.
        self.record_ordered_triggers_with_receipt(expected, triggers)
            .map(|receipt| receipt.into_parts().0)
    }
    pub(crate) fn record_speed_trigger(
        &self,
        actor: SeatId,
    ) -> Result<(Self, StackEntryId), StackWindowError> {
        let round = self.priority()?;
        let PriorityPhase::RequiresChecks { candidate } = round.phase() else {
            return Err(StackWindowError::ChecksNotPending);
        };
        self.validate_player(actor)?;
        let mut next = self.clone();
        let entry = next.allocate(actor, StackKind::Speed)?;
        let id = entry.id();
        next.stack.push(entry);
        next.status = Status::Priority(PriorityRound::new(self.order.clone(), candidate, 0)?);
        next.revision = self.next_revision(self.revision)?;
        Ok((next, id))
    }
    pub(crate) fn record_delayed_copy_trigger(
        &self,
        actor: SeatId,
        original: StackEntryId,
    ) -> Result<(Self, StackEntryId), StackWindowError> {
        let round = self.priority()?;
        let PriorityPhase::RequiresChecks { candidate } = round.phase() else {
            return Err(StackWindowError::ChecksNotPending);
        };
        self.validate_player(actor)?;
        let mut next = self.clone();
        let entry = next.allocate(actor, StackKind::DelayedCopy { original })?;
        let id = entry.id();
        next.stack.push(entry);
        next.status = Status::Priority(PriorityRound::new(self.order.clone(), candidate, 0)?);
        next.revision = self.next_revision(self.revision)?;
        Ok((next, id))
    }
    pub(crate) fn record_monarch_trigger(
        &self,
        actor: SeatId,
    ) -> Result<(Self, StackEntryId), StackWindowError> {
        let round = self.priority()?;
        let PriorityPhase::RequiresChecks { candidate } = round.phase() else {
            return Err(StackWindowError::ChecksNotPending);
        };
        self.validate_player(actor)?;
        let mut next = self.clone();
        let entry = next.allocate(actor, StackKind::Monarch)?;
        let id = entry.id();
        next.stack.push(entry);
        next.status = Status::Priority(PriorityRound::new(self.order.clone(), candidate, 0)?);
        next.revision = self.next_revision(self.revision)?;
        Ok((next, id))
    }
    pub(crate) fn record_emblem_trigger(
        &self,
        actor: SeatId,
        emblem: crate::emblem::EmblemId,
    ) -> Result<(Self, StackEntryId), StackWindowError> {
        let round = self.priority()?;
        let PriorityPhase::RequiresChecks { candidate } = round.phase() else {
            return Err(StackWindowError::ChecksNotPending);
        };
        self.validate_player(actor)?;
        let mut next = self.clone();
        let entry = next.allocate(actor, StackKind::Emblem { emblem })?;
        let id = entry.id();
        next.stack.push(entry);
        next.status = Status::Priority(PriorityRound::new(self.order.clone(), candidate, 0)?);
        next.revision = self.next_revision(self.revision)?;
        Ok((next, id))
    }
    pub(crate) fn record_ordered_triggers_with_receipt(
        &self,
        expected: u64,
        triggers: Vec<OrderedTrigger>,
    ) -> Result<OrderedTriggerReceipt, StackWindowError> {
        let revision = self.next_revision(expected)?;
        let round = self.priority()?;
        let PriorityPhase::RequiresChecks { candidate } = round.phase() else {
            return Err(StackWindowError::ChecksNotPending);
        };
        if triggers.is_empty() {
            return Err(StackWindowError::EmptyTriggerBatch);
        }
        for trigger in &triggers {
            self.validate_player(trigger.controller)?;
            self.validate_source(&trigger.source)?;
        }
        let mut next = self.clone();
        let mut entries = Vec::with_capacity(triggers.len());
        for trigger in triggers {
            let entry = next.allocate(
                trigger.controller,
                StackKind::TriggeredAbility {
                    source: trigger.source,
                },
            )?;
            entries.push(entry.id());
            next.stack.push(entry);
        }
        next.status = Status::Priority(PriorityRound::new(self.order.clone(), candidate, 0)?);
        next.revision = revision;
        Ok(OrderedTriggerReceipt {
            window: next,
            entries,
        })
    }
    /// Select the alternate characteristics in an isolated prospective cast window.
    pub(crate) fn select_siege_back(&mut self, spell: ObjectId) -> Result<(), StackWindowError> {
        Ok(self.cards.select_reverse_spell(spell)?)
    }
    pub(crate) fn select_prototype_spell(
        &mut self,
        spell: ObjectId,
    ) -> Result<(), StackWindowError> {
        Ok(self.cards.select_prototype(spell)?)
    }
    pub(crate) fn enter_transformed(&mut self, id: ObjectId) -> Result<(), StackWindowError> {
        Ok(self.cards.enter_transformed(id)?)
    }
    pub(crate) fn preview_adventure(&mut self, spell: ObjectId) -> Result<(), StackWindowError> {
        Ok(self.cards.select_adventure(spell)?)
    }
    /// Install the selected spell characteristics on the admitted stack object.
    pub(crate) fn select_split_half(
        &mut self,
        object: ObjectId,
        half: crate::program::SplitHalf,
    ) -> Result<(), StackWindowError> {
        Ok(self.cards.select_split_half(object, half)?)
    }
    pub(crate) fn select_adventure_spell(
        &mut self,
        entry: StackEntryId,
    ) -> Result<(), StackWindowError> {
        let Some(StackKind::CardSpell(spell)) = self
            .stack
            .last()
            .filter(|top| top.id() == entry)
            .map(StackEntry::kind)
        else {
            return Err(StackWindowError::InconsistentStack);
        };
        Ok(self.cards.select_adventure(*spell)?)
    }
    /// Install the departure replacement on the returned battlefield incarnation.
    pub(crate) fn mark_corpse_return(&mut self, id: ObjectId) -> Result<(), StackWindowError> {
        Ok(self.cards.mark_corpse_return(id)?)
    }
    pub(crate) fn mark_unearthed(&mut self, id: ObjectId) -> Result<(), StackWindowError> {
        Ok(self.cards.mark_unearthed(id)?)
    }
    /// One already-adjudicated resolution instruction, not an effect interpreter.
    /// Moving a card spell removes its stack entry, but a resolving continuation
    /// survives its own card's exit (CR 608.2m). Casting another physical card during
    /// resolution uses a trigger-granted cast permission; spell copies use their explicit method.
    pub fn record_resolution_move(
        &self,
        expected: u64,
        card: ObjectId,
        destination: ObjectDestination,
    ) -> Result<Self, StackWindowError> {
        Ok(self
            .resolution_move_with_exit(expected, card, destination)?
            .0)
    }
    pub(crate) fn resolution_move_with_exit(
        &self,
        expected: u64,
        card: ObjectId,
        destination: ObjectDestination,
    ) -> Result<(Self, StackExit), StackWindowError> {
        let revision = self.next_revision(expected)?;
        if !matches!(self.status, Status::Resolving { .. }) {
            return Err(StackWindowError::NotResolving);
        }
        match destination {
            ObjectDestination::Stack { .. } => {
                return Err(StackWindowError::UnsupportedStackInsertion);
            }
            ObjectDestination::Battlefield { controller } => self.validate_player(controller)?,
            _ => {}
        }
        self.move_card(revision, card, destination)
    }
    /// Move distinct selected cards from the searched library in one batch. The entry controller may differ from its owner.
    pub(crate) fn record_library_search_moves(
        &self,
        expected: u64,
        entry: StackEntryId,
        player: SeatId,
        entry_controller: SeatId,
        moves: &[(ObjectId, ObjectDestination)],
    ) -> Result<(Self, Vec<ObjectMovement>), StackWindowError> {
        let revision = self.next_revision(expected)?;
        if self.resolving().is_none_or(|value| value.id() != entry) {
            return Err(StackWindowError::WrongResolution);
        }
        let mut unique = BTreeSet::new();
        for &(id, destination) in moves {
            if !unique.insert(id)
                || self.cards.get(id).is_none_or(|object| {
                    object.zone() != ObjectZone::Library(player) || object.card_id().is_none()
                })
                || !(matches!(
                    destination,
                    ObjectDestination::Hand
                        | ObjectDestination::Graveyard
                        | ObjectDestination::Exile
                ) || matches!(destination, ObjectDestination::Battlefield { controller } if controller == entry_controller && self.validate_player(controller).is_ok()))
            {
                return Err(StackWindowError::InvalidResolutionMove);
            }
        }
        self.apply_recorded_zone_moves(revision, moves)
    }
    /// Trusted replacement-adjusted state-based zone moves, only while CR 117.5
    /// checks are pending. No priority is granted and no triggers are discovered.
    /// Rows and evidence use application order; top arrivals reverse that order
    /// within each pile. Any rejection rolls back the complete batch.
    pub fn record_check_moves(
        &self,
        expected: u64,
        moves: Vec<(ObjectId, ObjectDestination)>,
    ) -> Result<(Self, Vec<ObjectMovement>), StackWindowError> {
        let revision = self.next_revision(expected)?;
        self.require_checks()?;
        if moves.is_empty() {
            return Err(StackWindowError::EmptyCheckMoves);
        }
        let mut sources = BTreeSet::new();
        for &(card, destination) in &moves {
            if self.cards.get(card).map(GameObject::zone) != Some(ObjectZone::Battlefield)
                || !sources.insert(card)
            {
                return Err(StackWindowError::InvalidCheckMove);
            }
            match destination {
                ObjectDestination::Stack { .. } | ObjectDestination::Battlefield { .. } => {
                    return Err(StackWindowError::InvalidCheckMove);
                }
                _ => {}
            }
        }
        self.apply_recorded_zone_moves(revision, &moves)
    }
    /// Destroy an Aura with Umbra armor (CR 702.89a) during destruction replacement resolution.
    pub(crate) fn destroy_umbra_aura(
        &self,
        aura: ObjectId,
    ) -> Result<(Self, Option<ObjectMovement>), StackWindowError> {
        if self.cards.get(aura).map(GameObject::zone) != Some(ObjectZone::Battlefield) {
            return Ok((self.clone(), None));
        }
        let revision = self.next_revision(self.revision())?;
        let (next, mut moves) =
            self.apply_recorded_zone_moves(revision, &[(aura, ObjectDestination::Graveyard)])?;
        Ok((next, moves.pop()))
    }
    /// Apply the eligible commanders selected by their owners during CR 903.9a checks.
    pub(crate) fn return_commanders(
        &self,
        selected: &[ObjectId],
    ) -> Result<(Self, Vec<ObjectMovement>), StackWindowError> {
        self.require_checks()?;
        let mut seen = BTreeSet::new();
        for id in selected {
            if !seen.insert(*id)
                || !self.cards.get(*id).is_some_and(|card| {
                    matches!(card.zone(), ObjectZone::Graveyard(_) | ObjectZone::Exile)
                })
            {
                return Err(StackWindowError::InvalidCheckMove);
            }
        }
        let application = selected
            .iter()
            .map(|id| (*id, ObjectDestination::Command))
            .collect::<Vec<_>>();
        self.apply_recorded_zone_moves(self.next_revision(self.revision())?, &application)
    }
    /// Actual cleanup work after the rules layer validates the required subset.
    /// Selection and evidence are final top-to-bottom entrant order above the old
    /// owner graveyard; top insertion therefore applies the selected rows in reverse.
    pub(crate) fn record_cleanup_discards(
        &self,
        expected: u64,
        player: SeatId,
        selected_top_to_bottom: &[ObjectId],
    ) -> Result<(Self, Vec<ObjectMovement>), StackWindowError> {
        let application = selected_top_to_bottom
            .iter()
            .rev()
            .map(|id| (*id, crate::program::PublicZoneDestination::Graveyard))
            .collect::<Vec<_>>();
        let (window, mut movements) = self.record_cleanup_moves(expected, player, &application)?;
        movements.reverse();
        Ok((window, movements))
    }
    /// Actual rules-authorized mixed cleanup departures; uses the same atomic registry owner.
    pub(crate) fn record_cleanup_moves(
        &self,
        expected: u64,
        player: SeatId,
        application: &[(ObjectId, crate::program::PublicZoneDestination)],
    ) -> Result<(Self, Vec<ObjectMovement>), StackWindowError> {
        let revision = self.next_revision(expected)?;
        if !self.stack.is_empty() || !matches!(self.status, Status::TurnWork) {
            return Err(StackWindowError::NotInTurnWork);
        }
        self.validate_player(player)?;
        if player != self.active {
            return Err(StackWindowError::WrongController);
        }
        let mut selected = BTreeSet::new();
        for &(object, _) in application {
            if self.cards.get(object).map(GameObject::zone) != Some(ObjectZone::Hand(player))
                || !selected.insert(object)
            {
                return Err(StackWindowError::InvalidCheckMove);
            }
        }
        let moves = application
            .iter()
            .map(|(id, destination)| {
                Ok((
                    *id,
                    destination
                        .physical(None)
                        .ok_or(StackWindowError::InvalidCheckMove)?,
                ))
            })
            .collect::<Result<Vec<_>, StackWindowError>>()?;
        self.apply_recorded_zone_moves(revision, &moves)
    }
    /// Apply one complete resolving instruction's selected batch. This never
    /// supplies checks or cleanup authority. All source conditions and distinct
    /// identities are checked before the registry's atomic application.
    ///
    /// # Errors
    /// Rejects a stale/exhausted revision, absent or different resolving entry,
    /// empty batch, foreign participant, repeated identity, wrong source zone,
    /// controller or physical-card origin. Propagates atomic registry errors.
    pub(crate) fn record_resolution_selection_moves(
        &self,
        expected: u64,
        entry: StackEntryId,
        selected: Vec<ResolutionSelectionMove>,
    ) -> Result<(Self, Vec<ObjectMovement>), StackWindowError> {
        let revision = self.next_revision(expected)?;
        let resolving = self.resolving().ok_or(StackWindowError::NotResolving)?;
        if resolving.id() != entry {
            return Err(StackWindowError::WrongResolution);
        }
        if selected.is_empty() {
            return Err(StackWindowError::EmptyResolutionMoves);
        }
        let mut unique = BTreeSet::new();
        let mut moves = Vec::with_capacity(selected.len());
        for selection in selected {
            let (player, id, destination) = match selection {
                ResolutionSelectionMove::Sacrifice {
                    player,
                    object,
                    destination,
                }
                | ResolutionSelectionMove::GraveyardReturn {
                    player,
                    object,
                    destination,
                }
                | ResolutionSelectionMove::BattlefieldEffect {
                    player,
                    object,
                    destination,
                }
                | ResolutionSelectionMove::FromHand {
                    player,
                    object,
                    destination,
                } => (player, object, destination),
            };
            self.validate_player(player)?;
            let object = self
                .cards
                .get(id)
                .ok_or(StackWindowError::InvalidResolutionMove)?;
            let eligible = match selection {
                ResolutionSelectionMove::BattlefieldEffect { .. }
                | ResolutionSelectionMove::Sacrifice { .. } => {
                    object.zone() == ObjectZone::Battlefield && object.controller() == Some(player)
                }
                ResolutionSelectionMove::GraveyardReturn { .. } => {
                    object.card_id().is_some() && object.zone() == ObjectZone::Graveyard(player)
                }
                ResolutionSelectionMove::FromHand { .. } => {
                    object.card_id().is_some() && object.zone() == ObjectZone::Hand(player)
                }
            };
            if !eligible || !unique.insert(id) {
                return Err(StackWindowError::InvalidResolutionMove);
            }
            let controller = (destination == crate::program::PublicZoneDestination::Battlefield)
                .then_some(object.owner());
            let physical = destination
                .physical(controller)
                .ok_or(StackWindowError::InvalidResolutionMove)?;
            moves.push((id, physical));
        }
        self.apply_recorded_zone_moves(revision, &moves)
    }
    /// Commit one rules-authorized frozen library partition through the same atomic
    /// physical movement owner. Reordering surviving cards never renews identity.
    pub(crate) fn record_library_top(
        &self,
        expected: u64,
        entry: StackEntryId,
        commit: LibraryTopCommit<'_>,
    ) -> Result<(Self, Vec<ObjectMovement>), StackWindowError> {
        if self.resolving().map(|e| e.id()) != Some(entry) {
            return Err(StackWindowError::WrongResolution);
        }
        self.record_revealed_top(expected, commit)
    }
    /// Commit an exact revealed partition at a draw cut or a resolving instruction.
    pub(crate) fn record_revealed_top(
        &self,
        expected: u64,
        commit: LibraryTopCommit<'_>,
    ) -> Result<(Self, Vec<ObjectMovement>), StackWindowError> {
        let LibraryTopCommit {
            player,
            frozen,
            top,
            bottom,
            moves_application,
        } = commit;
        let revision = self.next_revision(expected)?;
        self.validate_player(player)?;
        let library = self.cards.library(player);
        let partition: Vec<_> = top
            .iter()
            .chain(bottom)
            .copied()
            .chain(moves_application.iter().map(|movement| movement.before))
            .collect();
        if !library.starts_with(frozen)
            || partition.len() != frozen.len()
            || partition.iter().copied().collect::<BTreeSet<_>>().len() != partition.len()
            || partition.iter().copied().collect::<BTreeSet<_>>()
                != frozen.iter().copied().collect()
            || frozen.iter().any(|id| {
                self.cards.get(*id).is_none_or(|c| {
                    c.zone() != ObjectZone::Library(player)
                        || c.owner() != player
                        || c.card_id().is_none()
                })
            })
        {
            return Err(StackWindowError::InvalidLibraryTop);
        }
        let order = top
            .iter()
            .chain(&library[frozen.len()..])
            .chain(bottom)
            .copied()
            .collect();
        let moves: Vec<_> = moves_application
            .iter()
            .map(|movement| {
                Ok((
                    movement.before,
                    movement
                        .destination
                        .physical(None)
                        .ok_or(StackWindowError::InvalidLibraryTop)?,
                ))
            })
            .collect::<Result<_, StackWindowError>>()?;
        let (next, movements) = self.apply_recorded_zone_moves(revision, &moves)?;
        let cards = next
            .cards
            .reorder_library(next.cards.revision(), player, order)?;
        Ok((next.with_cards(revision, cards), movements))
    }
    /// Shared physical application only. Each caller retains its own legal
    /// phase, source, controller and empty-batch policy above this helper.
    pub(crate) fn record_linked_return_moves(
        &self,
        moves: &[(ObjectId, ObjectDestination)],
    ) -> Result<(Self, Vec<ObjectMovement>), StackWindowError> {
        self.apply_recorded_zone_moves(self.next_revision(self.revision())?, moves)
    }
    /// Move one object to a non-stack destination during an inline event-replacement body (CR 614.1).
    pub(crate) fn record_inline_replacement_move(
        &self,
        card: ObjectId,
        destination: ObjectDestination,
    ) -> Result<(Self, Option<ObjectMovement>), StackWindowError> {
        if self.cards.get(card).is_none() {
            return Ok((self.clone(), None));
        }
        match destination {
            ObjectDestination::Stack { .. } => {
                return Err(StackWindowError::UnsupportedStackInsertion);
            }
            ObjectDestination::Battlefield { controller } => self.validate_player(controller)?,
            _ => {}
        }
        let revision = self.next_revision(self.revision())?;
        let (next, mut moves) = self.apply_recorded_zone_moves(revision, &[(card, destination)])?;
        Ok((next, moves.pop()))
    }
    fn apply_recorded_zone_moves(
        &self,
        revision: u64,
        moves: &[(ObjectId, ObjectDestination)],
    ) -> Result<(Self, Vec<ObjectMovement>), StackWindowError> {
        let (cards, movements) = self.cards.apply_zone_moves(self.cards.revision(), moves)?;
        let movements = movements
            .into_iter()
            .map(crate::object_identity::ZoneMoveOutcome::into_moved)
            .collect::<Result<Vec<_>, _>>()?;
        Ok((self.with_cards(revision, cards), movements))
    }
    fn move_card(
        &self,
        revision: u64,
        card: ObjectId,
        destination: ObjectDestination,
    ) -> Result<(Self, StackExit), StackWindowError> {
        let before = self
            .cards
            .get(card)
            .ok_or(ObjectIdentityError::UnknownObject)?
            .clone();
        let replacement = self
            .entries()
            .iter()
            .chain(self.resolving())
            .find_map(|entry| {
                (before.zone() == ObjectZone::Stack
                    && matches!(entry.kind,StackKind::CardSpell(id) if id==card))
                .then_some(entry.exile_cast)
                .flatten()
            });
        let destination = replacement.map_or(destination, |kind| kind.destination(destination));
        let moved = self
            .cards
            .apply_zone_move(
                self.cards.revision(),
                card,
                before.unearth_destination(destination),
            )?
            .require_moved()?;
        let mut next = self.with_cards(revision, moved.next);
        next.stack
            .retain(|entry| !matches!(entry.kind,StackKind::CardSpell(object) if object==card));
        Ok((
            next,
            StackExit::Object(ObjectMovement {
                before,
                after: moved.after,
            }),
        ))
    }
    /// CR 724.1b removes every entry together, retaining card and copy incarnations in exile.
    pub(crate) fn exile_stack_to_end_turn(
        &self,
        entry: StackEntryId,
    ) -> Result<(Self, Vec<StackExit>), StackWindowError> {
        if self.resolving().is_none_or(|actual| actual.id() != entry) {
            return Err(StackWindowError::WrongResolution);
        }
        let revision = self.next_revision(self.revision)?;
        let mut next = self.clone();
        let mut exits = Vec::new();
        for item in &self.stack {
            match item.kind() {
                StackKind::CardSpell(id) => {
                    let (state, exit) = next.move_card(revision, *id, ObjectDestination::Exile)?;
                    next = state;
                    exits.push(exit);
                }
                StackKind::SpellCopy { .. } => {
                    let old = self
                        .copies
                        .get(&item.id())
                        .ok_or(StackWindowError::InconsistentStack)?;
                    if old.permanent_object.is_some() {
                        next = next.move_copy(revision, item.id(), ObjectDestination::Exile)?;
                    } else {
                        let copy = next
                            .copies
                            .get_mut(&item.id())
                            .ok_or(StackWindowError::InconsistentStack)?;
                        copy.zone = ObjectZone::Exile;
                        copy.controller = None;
                    }
                    let after = next
                        .copies
                        .get(&item.id())
                        .ok_or(StackWindowError::InconsistentStack)?;
                    exits.push(self.copy_exit(&next, old, after)?);
                }
                _ => {}
            }
        }
        next.stack.clear();
        next.status = Status::Priority(PriorityRound::new(
            self.order.clone(),
            self.step_priority_candidate()?,
            0,
        )?);
        next.revision = revision;
        Ok((next, exits))
    }
    /// End the current step once the mandatory CR 724.1c checks finish.
    pub(crate) fn finish_end_turn_checks(&self) -> Result<Self, StackWindowError> {
        self.require_checks()?;
        if !self.stack.is_empty() {
            return Err(StackWindowError::InconsistentStack);
        }
        Ok(Self {
            status: Status::StepEnded,
            revision: self.next_revision(self.revision)?,
            ..self.clone()
        })
    }
    /// Trusted completion after all resolution work/choices have finished. A card
    /// still on the stack requires its computed exit; an already departed card or
    /// an ability requires None. Never follow a stable card key to move a new object.
    /// Active-player priority is still blocked on CR 117.5 checks (CR 117.3b).
    /// Receipt-dropping raw primitive; the executor consumes the actual exit receipt.
    pub fn finish_resolution(
        &self,
        expected: u64,
        entry: StackEntryId,
        exit: Option<ObjectDestination>,
    ) -> Result<Self, StackWindowError> {
        Ok(self.finish_resolution_with_exit(expected, entry, exit)?.0)
    }
    pub(crate) fn finish_resolution_with_exit(
        &self,
        expected: u64,
        entry: StackEntryId,
        exit: Option<ObjectDestination>,
    ) -> Result<(Self, Option<StackExit>), StackWindowError> {
        let revision = self.next_revision(expected)?;
        let Status::Resolving { entry: pending, .. } = &self.status else {
            return Err(StackWindowError::NotResolving);
        };
        self.complete_entry(revision, pending, entry, exit)
    }
    fn complete_entry(
        &self,
        revision: u64,
        pending: &StackEntry,
        entry: StackEntryId,
        exit: Option<ObjectDestination>,
    ) -> Result<(Self, Option<StackExit>), StackWindowError> {
        if pending.id != entry {
            return Err(StackWindowError::WrongResolution);
        }
        let mut next = self.clone();
        let mut receipt = None;
        match pending.kind {
            StackKind::CardSpell(object) => {
                if let Some(card) = self.cards.get(object) {
                    if card.zone() != ObjectZone::Stack {
                        return Err(StackWindowError::InconsistentStack);
                    }
                    let destination = exit.ok_or(StackWindowError::MissingSpellExit)?;
                    match destination {
                        ObjectDestination::Stack { .. } => {
                            return Err(StackWindowError::UnsupportedStackInsertion);
                        }
                        ObjectDestination::Battlefield { controller } => {
                            self.validate_player(controller)?
                        }
                        _ => {}
                    }
                    let destination = pending
                        .exile_cast
                        .map_or(destination, |kind| kind.destination(destination));
                    let moved = self
                        .cards
                        .apply_zone_move(self.cards.revision(), object, destination)?
                        .require_moved()?;
                    receipt = Some(StackExit::Object(ObjectMovement {
                        before: card.clone(),
                        after: moved.after,
                    }));
                    next.cards = moved.next;
                } else if exit.is_some() {
                    return Err(StackWindowError::UnexpectedSpellExit);
                }
            }
            StackKind::SpellCopy { .. }
                if exit.is_none()
                    && !self.copies.contains_key(&entry)
                    && !self.stack.iter().any(|item| item.id == entry) => {}
            StackKind::SpellCopy { .. }
                if exit.is_none()
                    && self.copies.get(&entry).is_some_and(|copy| {
                        copy.zone == ObjectZone::Battlefield
                            && copy
                                .permanent_object
                                .and_then(|id| self.cards.get(id))
                                .is_some_and(|host| host.times_mutated() > 0)
                    }) =>
            {
                next.copies.remove(&entry);
            }
            StackKind::SpellCopy { .. } => {
                let before = self
                    .copies
                    .get(&entry)
                    .ok_or(StackWindowError::InconsistentStack)?
                    .clone();
                next = self.move_copy(
                    revision,
                    entry,
                    exit.ok_or(StackWindowError::MissingSpellExit)?,
                )?;
                let after = next
                    .copies
                    .get(&entry)
                    .ok_or(StackWindowError::InconsistentStack)?
                    .clone();
                receipt = Some(self.copy_exit(&next, &before, &after)?);
                if after.zone == ObjectZone::Battlefield {
                    next.copies.remove(&entry);
                }
            }
            _ => {
                if exit.is_some() {
                    return Err(StackWindowError::UnexpectedSpellExit);
                }
            }
        }
        next.stack.retain(|item| item.id != entry);
        next.status = Status::Priority(PriorityRound::new(
            self.order.clone(),
            self.step_priority_candidate()?,
            0,
        )?);
        next.revision = revision;
        Ok((next, receipt))
    }
    pub fn checking_targets(&self) -> Option<&StackEntry> {
        match &self.status {
            Status::CheckingTargets(entry) => Some(entry),
            _ => None,
        }
    }
    pub fn target_assessment(&self) -> Option<&TargetAssessment> {
        match &self.status {
            Status::Resolving { targets, .. } | Status::AllTargetsIllegal { targets, .. } => {
                Some(targets)
            }
            _ => None,
        }
    }
    pub(crate) fn bind_targets(
        &mut self,
        entry: StackEntryId,
        clauses: Vec<TargetClause>,
        choices: Vec<TargetChoice>,
    ) -> Result<(), StackWindowError> {
        let plan = TargetPlan::capture(self, entry, clauses, choices)?;
        self.stack
            .iter_mut()
            .find(|item| item.id == entry)
            .ok_or(StackWindowError::InconsistentStack)?
            .targets = plan;
        Ok(())
    }
    /// Guard both trusted placement operations with the same exact pending entry.
    fn validate_unbound_trigger(&self, entry: StackEntryId) -> Result<(), StackWindowError> {
        if !matches!(
            self.priority()?.phase(),
            PriorityPhase::RequiresChecks { .. }
        ) {
            return Err(StackWindowError::ChecksNotPending);
        }
        let actual = self
            .stack
            .last()
            .ok_or(StackWindowError::InconsistentStack)?;
        if actual.id() != entry
            || !matches!(
                actual.kind(),
                StackKind::TriggeredAbility { .. } | StackKind::Emblem { .. }
            )
            || !actual.targets().targets().is_empty()
        {
            return Err(StackWindowError::InconsistentStack);
        }
        Ok(())
    }
    /// Trusted placement binding; the executor owns legal-domain adjudication.
    pub(crate) fn bind_trigger_target(
        &self,
        expected: u64,
        entry: StackEntryId,
        clauses: Vec<TargetClause>,
        choices: Vec<TargetChoice>,
    ) -> Result<Self, StackWindowError> {
        let revision = self.next_revision(expected)?;
        self.validate_unbound_trigger(entry)?;
        if clauses.len() != 1 || clauses[0].minimum != 1 || clauses[0].maximum != 1 {
            return Err(TargetError::InvalidClause.into());
        }
        if choices.len() != 1 {
            return Err(TargetError::TargetCount.into());
        }
        let mut next = self.clone();
        next.bind_targets(entry, clauses, choices)?;
        next.revision = revision;
        Ok(next)
    }
    /// Bind the distinct choices to one printed multiple-target clause at trigger placement.
    pub(crate) fn bind_trigger_target_set(
        &self,
        expected: u64,
        entry: StackEntryId,
        clause: TargetClause,
        choices: Vec<TargetChoice>,
    ) -> Result<Self, StackWindowError> {
        let revision = self.next_revision(expected)?;
        self.validate_unbound_trigger(entry)?;
        let mut next = self.clone();
        next.bind_targets(entry, vec![clause], choices)?;
        next.revision = revision;
        Ok(next)
    }
    pub(crate) fn bind_trigger_mode_targets(
        &self,
        expected: u64,
        entry: StackEntryId,
        clauses: Vec<TargetClause>,
        choices: Vec<TargetChoice>,
    ) -> Result<Self, StackWindowError> {
        let revision = self.next_revision(expected)?;
        self.validate_unbound_trigger(entry)?;
        let mut next = self.clone();
        next.bind_targets(entry, clauses, choices)?;
        next.revision = revision;
        Ok(next)
    }
    /// Trusted CR 603.3d removal, preserving the check candidate and consumed ID.
    pub(crate) fn remove_unbound_trigger(
        &self,
        expected: u64,
        entry: StackEntryId,
    ) -> Result<Self, StackWindowError> {
        let revision = self.next_revision(expected)?;
        self.validate_unbound_trigger(entry)?;
        let mut next = self.clone();
        next.stack.pop();
        next.revision = revision;
        Ok(next)
    }
    /// Post-legality target capture, not casting or legal-domain discovery. Failure
    /// leaves the original window and its card location unchanged.
    pub fn record_targeted_card_spell(
        &self,
        expected: u64,
        actor: SeatId,
        card: ObjectId,
        clauses: Vec<TargetClause>,
        choices: Vec<TargetChoice>,
    ) -> Result<Self, StackWindowError> {
        let mut next = self.record_card_spell(expected, actor, card)?;
        let entry = next
            .stack
            .last()
            .ok_or(StackWindowError::InconsistentStack)?
            .id;
        next.bind_targets(entry, clauses, choices)?;
        Ok(next)
    }
    pub fn record_targeted_activation(
        &self,
        expected: u64,
        actor: SeatId,
        source: GameObject,
        clauses: Vec<TargetClause>,
        choices: Vec<TargetChoice>,
    ) -> Result<Self, StackWindowError> {
        let mut next = self.record_activation(expected, actor, source)?;
        let entry = next
            .stack
            .last()
            .ok_or(StackWindowError::InconsistentStack)?
            .id;
        next.bind_targets(entry, clauses, choices)?;
        Ok(next)
    }
    /// Remaining-rule checks are mandatory, exact-occurrence native inputs for this
    /// current revision. They cannot override disappeared identities. No effect work
    /// can begin before this gate; target legality is checked once at resolution start.
    pub fn check_resolution_targets(
        &self,
        expected: u64,
        entry: StackEntryId,
        checks: Vec<TargetCheck>,
    ) -> Result<Self, StackWindowError> {
        let revision = self.next_revision(expected)?;
        let Status::CheckingTargets(pending) = &self.status else {
            return Err(StackWindowError::TargetChecksNotPending);
        };
        if pending.id != entry {
            return Err(StackWindowError::WrongResolution);
        }
        let targets = pending.targets.assess(self, checks)?;
        let status = if targets.permission() == ResolutionPermission::AllTargetsIllegal {
            Status::AllTargetsIllegal {
                entry: pending.clone(),
                targets,
            }
        } else {
            Status::Resolving {
                entry: pending.clone(),
                targets,
            }
        };
        Ok(Self {
            revision,
            status,
            ..self.clone()
        })
    }
    pub(crate) fn merge_resolving_spell(
        &mut self,
        entry: StackEntryId,
        spell: ObjectId,
        host: ObjectId,
        on_top: bool,
        definition: crate::program::Program,
        top_face: crate::program::Program,
    ) -> Result<(), StackWindowError> {
        if self
            .resolving()
            .filter(|e| e.id() == entry)
            .and_then(|e| self.spell_object(e))
            != Some(spell)
        {
            return Err(StackWindowError::WrongResolution);
        }
        let revision = self.next_revision(self.revision)?;
        self.cards
            .merge_mutating_spell(spell, host, on_top, definition, top_face)?;
        if let Some(copy) = self.copies.get_mut(&entry) {
            copy.zone = ObjectZone::Battlefield;
            copy.permanent_object = Some(host);
        }
        self.revision = revision;
        Ok(())
    }
    /// Bestow and mutate explicitly continue resolving as an untargeted permanent
    /// spell when the only announced target is illegal (CR 702.103e and 702.140b).
    pub(crate) fn resolve_as_untargeted_permanent(
        &mut self,
        id: StackEntryId,
    ) -> Result<(), StackWindowError> {
        let Status::AllTargetsIllegal { entry, targets } = &self.status else {
            return Err(StackWindowError::NotResolving);
        };
        if entry.id != id || self.spell_object(entry).is_none() {
            return Err(StackWindowError::WrongResolution);
        }
        self.status = Status::Resolving {
            entry: entry.clone(),
            targets: targets.clone(),
        };
        Ok(())
    }
    /// All-illegal entries do not resolve. The enclosing kernel supplies the
    /// replacement-adjusted spell exit (normally owner's graveyard), or None for an
    /// ability. This gate cannot be used to run effects or silently skip checks.
    /// Receipt-dropping raw primitive; the executor consumes the actual exit receipt.
    pub fn finish_unresolved(
        &self,
        expected: u64,
        entry: StackEntryId,
        exit: Option<ObjectDestination>,
    ) -> Result<Self, StackWindowError> {
        Ok(self.finish_unresolved_with_exit(expected, entry, exit)?.0)
    }
    pub(crate) fn finish_unresolved_with_exit(
        &self,
        expected: u64,
        entry: StackEntryId,
        exit: Option<ObjectDestination>,
    ) -> Result<(Self, Option<StackExit>), StackWindowError> {
        let revision = self.next_revision(expected)?;
        let Status::AllTargetsIllegal { entry: pending, .. } = &self.status else {
            return Err(StackWindowError::TargetsNotRejected);
        };
        self.complete_entry(revision, pending, entry, exit)
    }
    /// Minimal binding-aware card-zone instruction. Illegal target occurrences never
    /// become an untargeted move or redirect to a returned card. Player/ability effect
    /// operations and do-as-much-as-possible interpretation are not implemented here.
    /// Receipt-dropping raw physical primitive; executor event publication uses its receipt path.
    pub fn record_targeted_resolution_move(
        &self,
        expected: u64,
        target: TargetId,
        destination: ObjectDestination,
    ) -> Result<Self, StackWindowError> {
        Ok(self
            .record_targeted_resolution_move_with_exit(expected, target, destination)?
            .0)
    }
    /// Counter exactly the spell or ability captured by the resolving ward instance.
    pub(crate) fn counter_captured_entry(
        &self,
        id: StackEntryId,
    ) -> Result<(Self, Option<StackExit>), StackWindowError> {
        let resolving = self.resolving().ok_or(StackWindowError::NotResolving)?;
        if id == resolving.id || !self.stack_precedes(id, resolving.id) {
            return Err(StackWindowError::IllegalTarget);
        }
        let entry = self
            .stack
            .iter()
            .find(|s| s.id == id)
            .ok_or(StackWindowError::NotSpell)?;
        let revision = self.next_revision(self.revision)?;
        match entry.kind() {
            StackKind::CardSpell(card) => {
                let (next, exit) = self.move_card(revision, *card, ObjectDestination::Graveyard)?;
                Ok((next, Some(exit)))
            }
            StackKind::SpellCopy { .. } => {
                let before = self
                    .copies
                    .get(&id)
                    .ok_or(StackWindowError::InconsistentStack)?
                    .clone();
                let next = self.move_copy(revision, id, ObjectDestination::Graveyard)?;
                let after = next
                    .copies
                    .get(&id)
                    .ok_or(StackWindowError::InconsistentStack)?
                    .clone();
                let exit = self.copy_exit(&next, &before, &after)?;
                Ok((next, Some(exit)))
            }
            StackKind::Speed
            | StackKind::Monarch
            | StackKind::DelayedCopy { .. }
            | StackKind::Emblem { .. }
            | StackKind::ActivatedAbility { .. }
            | StackKind::TriggeredAbility { .. } => {
                let mut next = self.clone();
                next.revision = revision;
                next.stack.retain(|s| s.id != id);
                Ok((next, None))
            }
        }
    }
    pub(crate) fn record_targeted_resolution_move_with_exit(
        &self,
        expected: u64,
        target: TargetId,
        destination: ObjectDestination,
    ) -> Result<(Self, StackExit), StackWindowError> {
        self.next_revision(expected)?;
        let Status::Resolving { targets, .. } = &self.status else {
            return Err(StackWindowError::NotResolving);
        };
        let result = targets.get(target).ok_or(StackWindowError::UnknownTarget)?;
        if result.legality != TargetLegality::Legal {
            return Err(StackWindowError::IllegalTarget);
        }
        let card = match result.choice.target {
            TargetRef::Object(card) => card,
            TargetRef::Stack(id) => match self
                .stack
                .iter()
                .find(|entry| entry.id == id)
                .map(|entry| &entry.kind)
            {
                Some(StackKind::CardSpell(card)) => *card,
                Some(StackKind::SpellCopy { .. }) => {
                    let revision = self.next_revision(expected)?;
                    let before = self
                        .copies
                        .get(&id)
                        .ok_or(StackWindowError::InconsistentStack)?
                        .clone();
                    let next = self.move_copy(revision, id, destination)?;
                    let after = next
                        .copies
                        .get(&id)
                        .ok_or(StackWindowError::InconsistentStack)?
                        .clone();
                    let exit = self.copy_exit(&next, &before, &after)?;
                    return Ok((next, exit));
                }
                _ => return Err(StackWindowError::UnsupportedTargetOperation),
            },
            TargetRef::Player(_) => return Err(StackWindowError::UnsupportedTargetOperation),
        };
        self.resolution_move_with_exit(expected, card, destination)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::object_identity::CardId;
    const A: SeatId = SeatId(0);
    const B: SeatId = SeatId(1);
    const C: SeatId = SeatId(2);
    #[test]
    fn draw_batch_exposes_each_actual_world_before_advancing_to_the_next_card() {
        let (window, _) = declaration_window();
        let accepted = window.clone();
        let mut batch = window
            .begin_draw_batch(window.revision(), A, 2.into())
            .unwrap();
        let mut expected_before = window.cards.clone();
        let mut movements = Vec::new();
        for (index, card) in [CardId(5), CardId(6)].into_iter().enumerate() {
            let DrawBatchProgress::Drawn {
                before,
                next,
                movement,
            } = batch.next_draw().unwrap()
            else {
                panic!("expected one physical draw cut");
            };
            assert_eq!(before, expected_before);
            assert_eq!(
                movement.before.card_id().expect("physical fixture card"),
                card
            );
            assert_eq!(before.get(movement.before.id()), Some(&movement.before));
            assert_eq!(before.library(A).first(), Some(&movement.before.id()));
            assert_eq!(movement.before.zone(), ObjectZone::Library(A));
            assert_eq!(movement.after.zone(), ObjectZone::Hand(A));
            assert_eq!(
                movement.after.card_id().expect("physical fixture card"),
                card
            );
            assert_ne!(movement.before.id(), movement.after.id());
            assert!(next.cards().get(movement.before.id()).is_none());
            assert_eq!(next.cards().get(movement.after.id()), Some(&movement.after));
            assert_eq!(
                next.cards().revision(),
                window.cards.revision() + index as u64 + 1
            );
            assert_eq!(next.cards().library(A), &before.library(A)[1..]);
            if index == 0 {
                let later = window.cards.current_identity(CardId(6)).unwrap();
                assert_eq!(next.cards().get(later), window.cards.get(later));
            }
            assert_eq!(window, accepted);
            expected_before = next.cards().clone();
            movements.push(movement);
            batch = next;
        }
        let DrawBatchProgress::Complete {
            window: next,
            journal,
        } = batch.next_draw().unwrap()
        else {
            panic!("expected completed draw batch");
        };
        assert_eq!(window, accepted);
        assert_eq!(next.cards, expected_before);
        assert_eq!(next.revision(), window.revision() + 1);
        assert_eq!(next.phase(), window.phase());
        assert_eq!(next.active, window.active);
        assert_eq!(next.order, window.order);
        assert_eq!(next.stack, window.stack);
        assert_eq!(next.copies, window.copies);
        assert_eq!(next.next_entry, window.next_entry);
        assert_eq!(journal.events.len(), 2);
        for (event, movement) in journal.events.iter().zip(movements) {
            assert_eq!(
                event,
                &DrawEvent::Drawn {
                    player: A,
                    before: movement.before,
                    after: movement.after,
                }
            );
        }
    }
    #[test]
    fn draw_batch_and_bulk_preserve_physical_revisions_and_exact_empty_remainder() {
        let (window, _) = declaration_window();
        let accepted = window.clone();
        let huge: RulesInteger = "100000000000000000000000000000000000000000000000000"
            .to_owned()
            .try_into()
            .unwrap();
        for amount in [0.into(), 1.into(), 2.into(), 3.into(), huge.clone()] {
            let primitive = window
                .cards
                .draw_cards(window.cards.revision(), A, amount.clone())
                .unwrap();
            let (next, journal) = window
                .draw_cards(window.revision(), A, amount.clone())
                .unwrap();
            assert_eq!(window, accepted);
            assert_eq!(next.cards, primitive.next);
            assert_eq!(journal, primitive.journal);
            assert_eq!(next.revision(), window.revision() + 1);
            let successes = if amount < 2.into() {
                usize::from(amount == 1.into())
            } else {
                2
            };
            assert_eq!(
                next.cards.revision(),
                window.cards.revision() + successes as u64
            );
            assert_eq!(
                journal.events.len(),
                successes + usize::from(amount > 2.into())
            );
            if amount > 2.into() {
                assert_eq!(
                    journal.events.last(),
                    Some(&DrawEvent::EmptyLibrary {
                        player: A,
                        attempts: amount.minus(&2.into()),
                    })
                );
            }
        }
        let mut batch = window.begin_draw_batch(window.revision(), A, huge).unwrap();
        for _ in 0..2 {
            let DrawBatchProgress::Drawn { next, .. } = batch.next_draw().unwrap() else {
                panic!("library still has a real card");
            };
            batch = next;
        }
        assert!(matches!(
            batch.next_draw().unwrap(),
            DrawBatchProgress::Complete { .. }
        ));
    }
    #[test]
    fn draw_batch_zero_and_empty_do_not_consume_exhausted_physical_allocators() {
        let (mut window, _) = declaration_window();
        window.cards.set_test_revision(u64::MAX);
        window.cards.set_test_next_identity(u64::MAX);
        let accepted = window.clone();
        let (zero, journal) = window.draw_cards(window.revision(), A, 0.into()).unwrap();
        assert_eq!(zero.cards, window.cards);
        assert_eq!(zero.revision(), window.revision() + 1);
        assert!(journal.events.is_empty());
        let huge: RulesInteger = "999999999999999999999999999999999999999999999999999"
            .to_owned()
            .try_into()
            .unwrap();
        let (empty, journal) = window
            .draw_cards(window.revision(), B, huge.clone())
            .unwrap();
        assert_eq!(window, accepted);
        assert_eq!(empty.cards, window.cards);
        assert_eq!(empty.revision(), window.revision() + 1);
        assert_eq!(
            journal.events,
            vec![DrawEvent::EmptyLibrary {
                player: B,
                attempts: huge,
            }]
        );
        assert_eq!(
            window.draw_cards(window.revision(), A, 1.into()),
            Err(StackWindowError::Objects(
                ObjectIdentityError::RevisionExhausted
            ))
        );
        assert_eq!(window, accepted);
    }
    #[test]
    fn draw_batch_preserves_owner_validation_order_even_without_physical_work() {
        let (window, _) = declaration_window();
        let accepted = window.clone();
        assert!(matches!(
            window.begin_draw_batch(window.revision() - 1, A, 0.into()),
            Err(StackWindowError::StaleRevision)
        ));
        assert_eq!(
            window.draw_cards(window.revision(), SeatId(99), (-1).into()),
            Err(StackWindowError::UnknownPlayer)
        );
        assert_eq!(
            window.draw_cards(window.revision(), A, (-1).into()),
            Err(StackWindowError::Objects(
                ObjectIdentityError::NegativeDrawCount
            ))
        );
        let priority = window.enter_turn_step(window.revision(), A, true).unwrap();
        assert_eq!(
            priority.draw_cards(priority.revision(), SeatId(99), (-1).into()),
            Err(StackWindowError::NotInTurnWork)
        );
        let mut exhausted = window.clone();
        exhausted.revision = u64::MAX;
        for player in [A, B, SeatId(99)] {
            assert_eq!(
                exhausted.draw_cards(u64::MAX, player, 0.into()),
                Err(StackWindowError::RevisionExhausted)
            );
        }
        assert_eq!(window, accepted);
    }
    #[test]
    fn draw_batch_late_fault_drops_the_prefix_and_preserves_the_accepted_window() {
        for identity_fault in [true, false] {
            let (mut window, _) = declaration_window();
            if identity_fault {
                window.cards.set_test_next_identity(u64::MAX - 1);
            } else {
                window.cards.set_test_revision(u64::MAX - 1);
            }
            let accepted = window.clone();
            let batch = window
                .begin_draw_batch(window.revision(), A, 2.into())
                .unwrap();
            let DrawBatchProgress::Drawn { next, movement, .. } = batch.next_draw().unwrap() else {
                panic!("first physical draw must succeed before the fault");
            };
            assert_eq!(
                movement.before.card_id().expect("physical fixture card"),
                CardId(5)
            );
            assert_eq!(next.cards().get(movement.after.id()), Some(&movement.after));
            let expected = StackWindowError::Objects(if identity_fault {
                ObjectIdentityError::IdentityExhausted
            } else {
                ObjectIdentityError::RevisionExhausted
            });
            assert_eq!(next.next_draw().unwrap_err(), expected);
            assert_eq!(window, accepted);
            assert_eq!(
                window.draw_cards(window.revision(), A, 2.into()),
                Err(expected)
            );
            assert_eq!(window, accepted);
        }
    }
    fn declaration_window() -> (StackWindow, [ObjectId; 4]) {
        let mut cards = GameObjects::new(0);
        for (card, owner, destination) in [
            (1, A, ObjectDestination::Battlefield { controller: A }),
            (2, B, ObjectDestination::Battlefield { controller: A }),
            (3, B, ObjectDestination::Battlefield { controller: B }),
            (4, A, ObjectDestination::Hand),
            (5, A, ObjectDestination::LibraryBottom),
            (6, A, ObjectDestination::LibraryBottom),
        ] {
            cards = cards
                .seed_card(cards.revision(), CardId(card), owner, destination)
                .unwrap()
                .next;
        }
        cards = cards.record_turn_start(cards.revision(), A).unwrap();
        let objects = [1, 2, 3, 4].map(|card| cards.current_identity(CardId(card)).unwrap());
        let window = StackWindow::new(vec![A, B, C], A, cards).unwrap();
        let window = window.enter_turn_step(window.revision(), A, false).unwrap();
        (window, objects)
    }
    fn discard_window() -> (StackWindow, [ObjectId; 3]) {
        let mut cards = GameObjects::new(0);
        for (card, owner, destination) in [
            (81, A, ObjectDestination::Hand),
            (2, A, ObjectDestination::Hand),
            (39, A, ObjectDestination::Hand),
            (12, A, ObjectDestination::Graveyard),
            (5, A, ObjectDestination::Graveyard),
            (6, B, ObjectDestination::Graveyard),
            (1, A, ObjectDestination::LibraryBottom),
            (91, B, ObjectDestination::Battlefield { controller: A }),
            (66, B, ObjectDestination::Hand),
        ] {
            cards = cards
                .seed_card(cards.revision(), CardId(card), owner, destination)
                .unwrap()
                .next;
        }
        let selected = [81, 2, 39].map(|card| cards.current_identity(CardId(card)).unwrap());
        let window = StackWindow::new(vec![B, A, C], A, cards).unwrap();
        (
            window.enter_turn_step(window.revision(), A, false).unwrap(),
            selected,
        )
    }
    fn graveyard_cards(window: &StackWindow, owner: SeatId) -> Vec<CardId> {
        window
            .cards
            .graveyard(owner)
            .iter()
            .map(|id| {
                window
                    .cards
                    .get(*id)
                    .unwrap()
                    .card_id()
                    .expect("physical fixture card")
            })
            .collect()
    }
    #[test]
    fn cleanup_subset_and_movement_records_use_selected_final_top_order() {
        let (window, [first, retained, last]) = discard_window();
        let selected = [last, first];
        let before = window.clone();
        let (next, movements) = window
            .record_cleanup_discards(window.revision(), A, &selected)
            .unwrap();
        assert_eq!(window, before);
        assert_eq!(next.revision(), window.revision() + 1);
        assert_eq!(next.cards.revision(), window.cards.revision() + 2);
        assert_eq!(next.phase(), WindowPhase::TurnWork);
        assert_eq!(next.active, A);
        assert_eq!(next.order, window.order);
        assert_eq!(next.stack, window.stack);
        assert_eq!(next.copies, window.copies);
        assert_eq!(next.next_entry, window.next_entry);
        assert_eq!(next.cards.library(A), window.cards.library(A));
        assert_eq!(next.cards.graveyard(B), window.cards.graveyard(B));
        assert_eq!(
            graveyard_cards(&next, A),
            vec![CardId(39), CardId(81), CardId(5), CardId(12)]
        );
        assert_eq!(&next.cards.graveyard(A)[2..], window.cards.graveyard(A));
        assert_eq!(next.cards.get(retained), window.cards.get(retained));
        assert_eq!(movements.len(), 2);
        assert!(movements[0].after.id() > movements[1].after.id());
        for (movement, selected) in movements.iter().zip(selected) {
            assert_eq!(movement.before, *window.cards.get(selected).unwrap());
            assert_eq!(movement.before.zone(), ObjectZone::Hand(A));
            assert_eq!(movement.after.zone(), ObjectZone::Graveyard(A));
            assert_eq!(movement.after.controller(), None);
            assert_ne!(movement.after.id(), selected);
            assert_eq!(next.cards.get(movement.after.id()), Some(&movement.after));
            assert!(next.cards.get(selected).is_none());
        }
    }
    #[test]
    fn cleanup_all_three_card_permutations_preserve_the_old_segment() {
        let (window, objects) = discard_window();
        for order in [
            [0, 1, 2],
            [0, 2, 1],
            [1, 0, 2],
            [1, 2, 0],
            [2, 0, 1],
            [2, 1, 0],
        ] {
            let selected = order.map(|index| objects[index]);
            let (next, movements) = window
                .record_cleanup_discards(window.revision(), A, &selected)
                .unwrap();
            assert_eq!(&next.cards.graveyard(A)[3..], window.cards.graveyard(A));
            assert_eq!(
                movements
                    .iter()
                    .map(|movement| movement.before.id())
                    .collect::<Vec<_>>(),
                selected
            );
            assert_eq!(
                &next.cards.graveyard(A)[..3],
                movements
                    .iter()
                    .map(|movement| movement.after.id())
                    .collect::<Vec<_>>()
            );
            assert_eq!(
                graveyard_cards(&next, A)[..3],
                selected.map(|id| window
                    .cards
                    .get(id)
                    .unwrap()
                    .card_id()
                    .expect("physical fixture card"))
            );
            assert_eq!(next.cards.revision(), window.cards.revision() + 3);
        }
    }
    #[test]
    fn cleanup_rejects_wrong_work_actor_and_exact_card_domains_atomically() {
        let (window, [first, _, last]) = discard_window();
        let opposing = window.cards.current_identity(CardId(66)).unwrap();
        let permanent = window.cards.current_identity(CardId(91)).unwrap();
        let graveyard = window.cards.current_identity(CardId(5)).unwrap();
        for (actor, selected, error) in [
            (SeatId(99), vec![], StackWindowError::UnknownPlayer),
            (B, vec![], StackWindowError::WrongController),
            (A, vec![first, first], StackWindowError::InvalidCheckMove),
            (A, vec![first, opposing], StackWindowError::InvalidCheckMove),
            (
                A,
                vec![first, permanent],
                StackWindowError::InvalidCheckMove,
            ),
            (
                A,
                vec![first, graveyard],
                StackWindowError::InvalidCheckMove,
            ),
        ] {
            let before = window.clone();
            assert_eq!(
                window.record_cleanup_discards(window.revision(), actor, &selected),
                Err(error)
            );
            assert_eq!(window, before);
        }
        assert_eq!(
            window.record_cleanup_discards(window.revision() - 1, A, &[first]),
            Err(StackWindowError::StaleRevision)
        );
        let (departed, _) = window
            .record_cleanup_discards(window.revision(), A, &[last])
            .unwrap();
        let before = departed.clone();
        assert_eq!(
            departed.record_cleanup_discards(departed.revision(), A, &[first, last]),
            Err(StackWindowError::InvalidCheckMove)
        );
        assert_eq!(departed, before);
        let priority = window.enter_turn_step(window.revision(), A, true).unwrap();
        assert_eq!(
            priority.record_cleanup_discards(priority.revision(), A, &[first]),
            Err(StackWindowError::NotInTurnWork)
        );
        let mut stacked = stacked_two();
        stacked.status = Status::TurnWork;
        assert_eq!(
            stacked.record_cleanup_discards(stacked.revision(), A, &[]),
            Err(StackWindowError::NotInTurnWork)
        );
    }
    #[test]
    fn cleanup_late_registry_failures_and_window_exhaustion_preserve_accepted_work() {
        for identity_fault in [true, false] {
            let (mut window, [first, _, last]) = discard_window();
            if identity_fault {
                window.cards.set_test_next_identity(u64::MAX - 1);
            } else {
                window.cards.set_test_revision(u64::MAX - 1);
            }
            let before = window.clone();
            assert_eq!(
                window.record_cleanup_discards(window.revision(), A, &[first, last]),
                Err(StackWindowError::Objects(if identity_fault {
                    ObjectIdentityError::IdentityExhausted
                } else {
                    ObjectIdentityError::RevisionExhausted
                }))
            );
            assert_eq!(window, before);
        }
        let (mut window, [first, _, _]) = discard_window();
        window.revision = u64::MAX;
        let before = window.clone();
        assert_eq!(
            window.record_cleanup_discards(u64::MAX, A, &[first]),
            Err(StackWindowError::RevisionExhausted)
        );
        assert_eq!(window, before);
    }
    #[test]
    fn empty_cleanup_batch_consumes_only_the_window_revision() {
        let (mut window, _) = discard_window();
        window.cards.set_test_revision(u64::MAX);
        window.cards.set_test_next_identity(u64::MAX);
        let before = window.clone();
        let (next, movements) = window
            .record_cleanup_discards(window.revision(), A, &[])
            .unwrap();
        assert_eq!(next.revision(), window.revision() + 1);
        assert_eq!(next.cards, window.cards);
        assert!(movements.is_empty());
        assert_eq!(window, before);
    }
    #[test]
    fn state_check_batch_preserves_application_order_owner_piles_and_stack_state() {
        let (mut window, [first, second, opposing, _]) = declaration_window();
        for (id, owner) in [(90, A), (80, B)] {
            window.cards = window
                .cards
                .seed_card(
                    window.cards.revision(),
                    CardId(id),
                    owner,
                    ObjectDestination::Graveyard,
                )
                .unwrap()
                .next;
        }
        let before = window.clone();
        let (next, movements) = window
            .record_check_moves(
                window.revision(),
                vec![
                    (second, ObjectDestination::Graveyard),
                    (first, ObjectDestination::Graveyard),
                    (opposing, ObjectDestination::Graveyard),
                ],
            )
            .unwrap();
        assert_eq!(window, before);
        assert_eq!(
            movements
                .iter()
                .map(|movement| movement.before.id())
                .collect::<Vec<_>>(),
            [second, first, opposing]
        );
        assert_eq!(graveyard_cards(&next, A), vec![CardId(1), CardId(90)]);
        assert_eq!(
            graveyard_cards(&next, B),
            vec![CardId(3), CardId(2), CardId(80)]
        );
        assert_eq!(next.revision(), window.revision() + 1);
        assert_eq!(next.cards.revision(), window.cards.revision() + 3);
        assert_eq!(next.cards.library(A), window.cards.library(A));
        assert_eq!(next.stack, window.stack);
        assert_eq!(next.copies, window.copies);
        assert_eq!(next.phase(), window.phase());
    }
    #[test]
    fn state_check_batch_rejects_invalid_late_rows_and_operational_faults_atomically() {
        let (window, [first, second, _, hand]) = declaration_window();
        for (last, destination, error) in [
            (
                hand,
                ObjectDestination::Graveyard,
                StackWindowError::InvalidCheckMove,
            ),
            (
                second,
                ObjectDestination::Battlefield { controller: A },
                StackWindowError::InvalidCheckMove,
            ),
            (
                second,
                ObjectDestination::Stack { controller: A },
                StackWindowError::InvalidCheckMove,
            ),
            (
                first,
                ObjectDestination::Exile,
                StackWindowError::InvalidCheckMove,
            ),
        ] {
            let before = window.clone();
            assert_eq!(
                window.record_check_moves(
                    window.revision(),
                    vec![(first, ObjectDestination::Graveyard), (last, destination)]
                ),
                Err(error)
            );
            assert_eq!(window, before);
        }
        assert_eq!(
            window.record_check_moves(window.revision(), vec![]),
            Err(StackWindowError::EmptyCheckMoves)
        );
        let priority = window
            .enter_turn_step(window.revision(), A, true)
            .unwrap()
            .finish_checks(window.revision() + 1)
            .unwrap();
        assert_eq!(
            priority.record_check_moves(
                priority.revision(),
                vec![(first, ObjectDestination::Graveyard)]
            ),
            Err(StackWindowError::ChecksNotPending)
        );
        for identity_fault in [true, false] {
            let mut exhausted = window.clone();
            if identity_fault {
                exhausted.cards.set_test_next_identity(u64::MAX - 1);
            } else {
                exhausted.cards.set_test_revision(u64::MAX - 1);
            }
            let before = exhausted.clone();
            assert_eq!(
                exhausted.record_check_moves(
                    exhausted.revision(),
                    vec![
                        (first, ObjectDestination::Graveyard),
                        (second, ObjectDestination::Graveyard)
                    ]
                ),
                Err(StackWindowError::Objects(if identity_fault {
                    ObjectIdentityError::IdentityExhausted
                } else {
                    ObjectIdentityError::RevisionExhausted
                }))
            );
            assert_eq!(exhausted, before);
        }
    }
    #[test]
    fn declaration_taps_are_physical_work_without_priority_or_stack_allocation() {
        let (mut window, [first, second, _, _]) = declaration_window();
        window.next_entry = u64::MAX;
        window.cards.set_test_next_identity(u64::MAX);
        let before = window.clone();
        let (next, changes) = window
            .record_declaration_taps(window.revision(), A, &[second, first])
            .unwrap();
        assert_eq!(window, before);
        assert_eq!(next.revision(), before.revision() + 1);
        assert_eq!(next.cards.revision(), before.cards.revision() + 2);
        assert_eq!(next.phase(), WindowPhase::TurnWork);
        assert_eq!(next.active_player(), A);
        assert_eq!(next.participants(), before.participants());
        assert_eq!(next.next_entry, u64::MAX);
        assert!(next.entries().is_empty());
        assert_eq!(next.copies, before.copies);
        assert_eq!(next.cards.library(A), before.cards.library(A));
        assert_eq!(changes.len(), 2);
        for (change, object) in changes.iter().zip([second, first]) {
            assert_eq!(change.before, *before.cards.get(object).unwrap());
            assert_eq!(change.after.id(), object);
            assert_eq!(change.after.owner(), change.before.owner());
            assert_eq!(change.after.controller(), Some(A));
            assert_eq!(change.after.tapped(), Some(true));
            assert_eq!(change.after.control_epoch(), change.before.control_epoch());
            assert_eq!(
                change.after.has_continuous_control_since_turn_start(),
                Some(true)
            );
            assert_eq!(next.cards.get(object), Some(&change.after));
        }
    }
    #[test]
    fn declaration_taps_reject_wrong_phase_actor_and_physical_sources_atomically() {
        let (window, [first, second, opposing, hand]) = declaration_window();
        for (actor, selected, expected) in [
            (SeatId(99), vec![], StackWindowError::UnknownPlayer),
            (B, vec![], StackWindowError::WrongController),
            (A, vec![first, opposing], StackWindowError::WrongController),
            (
                A,
                vec![first, hand],
                StackWindowError::Objects(ObjectIdentityError::NotPermanent),
            ),
            (
                A,
                vec![first, first],
                StackWindowError::Objects(ObjectIdentityError::StatusUnchanged),
            ),
        ] {
            let before = window.clone();
            assert_eq!(
                window.record_declaration_taps(window.revision(), actor, &selected),
                Err(expected)
            );
            assert_eq!(window, before);
        }
        assert_eq!(
            window.record_declaration_taps(window.revision() - 1, A, &[first]),
            Err(StackWindowError::StaleRevision)
        );
        let (tapped, _) = window
            .record_declaration_taps(window.revision(), A, &[second])
            .unwrap();
        let before = tapped.clone();
        assert_eq!(
            tapped.record_declaration_taps(tapped.revision(), A, &[first, second]),
            Err(StackWindowError::SourceTapped)
        );
        assert_eq!(tapped, before);
        let mut departed = window.clone();
        departed.cards = departed
            .cards
            .apply_zone_move(departed.cards.revision(), second, ObjectDestination::Hand)
            .unwrap()
            .require_moved()
            .unwrap()
            .next;
        let before = departed.clone();
        assert_eq!(
            departed.record_declaration_taps(departed.revision(), A, &[first, second]),
            Err(StackWindowError::Objects(
                ObjectIdentityError::UnknownObject
            ))
        );
        assert_eq!(departed, before);
        let priority = window.enter_turn_step(window.revision(), A, true).unwrap();
        assert_eq!(
            priority.record_declaration_taps(priority.revision(), A, &[first]),
            Err(StackWindowError::NotInTurnWork)
        );
        let mut stacked = stacked_two();
        stacked.status = Status::TurnWork;
        assert_eq!(
            stacked.record_declaration_taps(stacked.revision(), A, &[]),
            Err(StackWindowError::NotInTurnWork)
        );
    }
    #[test]
    fn declaration_second_tap_exhaustion_preserves_the_accepted_window() {
        let (mut window, [first, second, _, _]) = declaration_window();
        window.cards.set_test_revision(u64::MAX - 1);
        let before = window.clone();
        assert_eq!(
            window.record_declaration_taps(window.revision(), A, &[first, second]),
            Err(StackWindowError::Objects(
                ObjectIdentityError::RevisionExhausted
            ))
        );
        assert_eq!(window, before);
        assert_eq!(window.cards.get(first).unwrap().tapped(), Some(false));
        assert_eq!(window.cards.get(second).unwrap().tapped(), Some(false));
        window.revision = u64::MAX;
        let before = window.clone();
        assert_eq!(
            window.record_declaration_taps(u64::MAX, A, &[]),
            Err(StackWindowError::RevisionExhausted)
        );
        assert_eq!(window, before);
    }
    #[test]
    fn empty_declaration_and_zero_change_untap_preserve_exhausted_registry() {
        let (mut window, _) = declaration_window();
        window.cards.set_test_revision(u64::MAX);
        let before = window.clone();
        let (declared, changes) = window
            .record_declaration_taps(window.revision(), A, &[])
            .unwrap();
        assert!(changes.is_empty());
        assert_eq!(declared.cards, before.cards);
        assert_eq!(declared.revision(), before.revision() + 1);
        assert_eq!(declared.phase(), WindowPhase::TurnWork);
        let untapped = window.untap_active(window.revision()).unwrap();
        assert_eq!(untapped.cards, before.cards);
        assert_eq!(untapped.revision(), before.revision() + 1);
        assert_eq!(untapped.phase(), WindowPhase::TurnWork);
        assert_eq!(window, before);
    }
    #[test]
    fn untap_batch_keeps_per_card_revisions_and_controller_selection() {
        let (window, [first, second, opposing, _]) = declaration_window();
        let (mut tapped, _) = window
            .record_declaration_taps(window.revision(), A, &[first, second])
            .unwrap();
        tapped.cards = tapped
            .cards
            .set_tapped(tapped.cards.revision(), opposing, true)
            .unwrap()
            .next;
        let before = tapped.clone();
        let untapped = tapped.untap_active(tapped.revision()).unwrap();
        assert_eq!(untapped.cards.revision(), before.cards.revision() + 2);
        assert_eq!(untapped.revision(), before.revision() + 1);
        for object in [first, second] {
            assert_eq!(untapped.cards.get(object).unwrap().tapped(), Some(false));
            assert_eq!(
                untapped.cards.get(object).unwrap().control_epoch(),
                before.cards.get(object).unwrap().control_epoch()
            );
        }
        assert_eq!(untapped.cards.get(opposing), before.cards.get(opposing));
        assert_eq!(untapped.cards.library(A), before.cards.library(A));
        assert_eq!(tapped, before);
    }
    #[test]
    fn direct_resolution_and_state_check_moves_use_canonical_library_order() {
        for destination in [
            ObjectDestination::LibraryTop,
            ObjectDestination::LibraryBottom,
        ] {
            let mut cards = GameObjects::new(0);
            for (id, dest) in [
                (1, ObjectDestination::Hand),
                (2, ObjectDestination::LibraryBottom),
                (3, ObjectDestination::Battlefield { controller: A }),
            ] {
                cards = cards
                    .seed_card(cards.revision(), CardId(id), A, dest)
                    .unwrap()
                    .next;
            }
            let spell = cards.current_identity(CardId(1)).unwrap();
            let permanent = cards.current_identity(CardId(3)).unwrap();
            let initial = StackWindow::new(vec![A, B], A, cards).unwrap();
            let (checked, movements) = initial
                .record_check_moves(initial.revision(), vec![(permanent, destination)])
                .unwrap();
            assert_eq!(movements.len(), 1);
            assert_eq!(movements[0].before.id(), permanent);
            assert_eq!(
                checked
                    .cards
                    .library(A)
                    .iter()
                    .map(|id| checked
                        .cards
                        .get(*id)
                        .unwrap()
                        .card_id()
                        .expect("physical fixture card"))
                    .collect::<Vec<_>>(),
                if destination == ObjectDestination::LibraryTop {
                    vec![CardId(3), CardId(2)]
                } else {
                    vec![CardId(2), CardId(3)]
                }
            );
            let mut state = initial.finish_checks(initial.revision()).unwrap();
            state = state.record_card_spell(state.revision(), A, spell).unwrap();
            state = state.finish_checks(state.revision()).unwrap();
            state = state.pass(state.revision(), A).unwrap();
            state = state.finish_checks(state.revision()).unwrap();
            state = state.pass(state.revision(), B).unwrap();
            let entry = state.resolving().unwrap().id();
            let finished = state
                .finish_resolution(state.revision(), entry, Some(destination))
                .unwrap();
            assert_eq!(
                finished
                    .cards
                    .library(A)
                    .iter()
                    .map(|id| finished
                        .cards
                        .get(*id)
                        .unwrap()
                        .card_id()
                        .expect("physical fixture card"))
                    .collect::<Vec<_>>(),
                if destination == ObjectDestination::LibraryTop {
                    vec![CardId(1), CardId(2)]
                } else {
                    vec![CardId(2), CardId(1)]
                }
            );
        }
    }

    #[test]
    fn lifecycle_cannot_replace_open_priority_or_a_pending_stack_entry() {
        let (window, spell, _) = setup();
        let ready = window.finish_checks(window.revision()).unwrap();
        assert_eq!(
            ready.enter_turn_step(ready.revision(), B, false),
            Err(StackWindowError::NotInTurnWork)
        );
        let pending = ready.record_card_spell(ready.revision(), A, spell).unwrap();
        let before = pending.clone();
        assert_eq!(
            pending.enter_turn_step(pending.revision(), B, true),
            Err(StackWindowError::NotInTurnWork)
        );
        assert_eq!(pending, before);
        let mut resolving = pending.finish_checks(pending.revision()).unwrap();
        for actor in [A, B, C] {
            resolving = resolving.pass(resolving.revision(), actor).unwrap();
            if matches!(
                resolving.phase(),
                WindowPhase::Priority(PriorityPhase::RequiresChecks { .. })
            ) {
                resolving = resolving.finish_checks(resolving.revision()).unwrap();
            }
        }
        assert!(matches!(resolving.phase(), WindowPhase::Resolving(_)));
        let current = match resolving.resolving().unwrap().kind() {
            StackKind::CardSpell(object) => *object,
            _ => panic!("expected the real card spell"),
        };
        resolving = resolving
            .record_resolution_move(resolving.revision(), current, ObjectDestination::Exile)
            .unwrap();
        assert!(resolving.entries().is_empty());
        assert!(matches!(resolving.phase(), WindowPhase::Resolving(_)));
        let before = resolving.clone();
        assert_eq!(
            resolving.enter_turn_step(resolving.revision(), B, false),
            Err(StackWindowError::NotInTurnWork)
        );
        assert_eq!(resolving, before);
    }

    #[test]
    fn partial_untap_failure_preserves_all_physical_status_and_identity() {
        let mut cards = GameObjects::new(0);
        for card in [CardId(1), CardId(2)] {
            let added = cards
                .seed_card(
                    cards.revision(),
                    card,
                    A,
                    ObjectDestination::Battlefield { controller: A },
                )
                .unwrap();
            cards = added
                .next
                .set_tapped(added.next.revision(), added.after.id(), true)
                .unwrap()
                .next;
        }
        let window = StackWindow::new(vec![A, B], A, cards).unwrap();
        let mut window = window.enter_turn_step(window.revision(), A, false).unwrap();
        window.cards.set_test_revision(u64::MAX - 1);
        let before = window.clone();
        assert_eq!(
            window.untap_active(window.revision()),
            Err(StackWindowError::Objects(
                ObjectIdentityError::RevisionExhausted
            ))
        );
        assert_eq!(window, before);
        assert!(
            window
                .cards
                .iter()
                .all(|object| object.tapped() == Some(true))
        );
    }
    fn setup() -> (StackWindow, ObjectId, ObjectId) {
        let a = GameObjects::new(0)
            .seed_card(0, CardId(10), A, ObjectDestination::Hand)
            .unwrap();
        let b = a
            .next
            .seed_card(1, CardId(20), B, ObjectDestination::Hand)
            .unwrap();
        (
            StackWindow::new(vec![A, B, C], A, b.next).unwrap(),
            a.after.id(),
            b.after.id(),
        )
    }
    fn ready(window: StackWindow) -> StackWindow {
        window.finish_checks(window.revision()).unwrap()
    }
    fn passes(mut window: StackWindow, players: &[SeatId]) -> StackWindow {
        for player in players {
            if matches!(
                window.phase(),
                WindowPhase::Priority(PriorityPhase::RequiresChecks { .. })
            ) {
                window = ready(window);
            }
            window = window.pass(window.revision(), *player).unwrap();
        }
        window
    }
    fn pending(window: &StackWindow) -> StackEntryId {
        window.resolving().unwrap().id()
    }
    fn stacked_two() -> StackWindow {
        let (window, a, b) = setup();
        let window = ready(window);
        let window = window.record_card_spell(window.revision(), A, a).unwrap();
        let window = passes(window, &[A]);
        let window = ready(window);
        let window = window.record_card_spell(window.revision(), B, b).unwrap();
        passes(window, &[B, C, A])
    }
    #[test]
    fn all_players_must_pass_an_empty_stack_to_end_the_step() {
        for count in 2..=6 {
            for first in 0..count {
                let active = SeatId(first);
                let mut window = StackWindow::new(
                    (0..count).map(SeatId).collect(),
                    active,
                    GameObjects::new(0),
                )
                .unwrap();
                for offset in 0..count {
                    assert_eq!(
                        window.phase(),
                        WindowPhase::Priority(PriorityPhase::RequiresChecks {
                            candidate: SeatId((first + offset) % count)
                        })
                    );
                    window = ready(window);
                    window = window
                        .pass(window.revision(), SeatId((first + offset) % count))
                        .unwrap();
                    if offset + 1 < count {
                        assert_ne!(window.phase(), WindowPhase::StepEnded);
                    }
                }
                assert_eq!(window.phase(), WindowPhase::StepEnded);
                assert!(window.entries().is_empty());
                assert_eq!(
                    window.finish_checks(window.revision()),
                    Err(StackWindowError::NotInPriority)
                );
                assert_eq!(
                    window.pass(window.revision(), active),
                    Err(StackWindowError::NotInPriority)
                );
            }
        }
    }
    #[test]
    fn responses_resolve_last_in_first_out_and_restart_at_the_active_player() {
        let window = stacked_two();
        assert_eq!(window.entries().len(), 2);
        let lower = window.entries()[0].clone();
        let upper = window.entries()[1].clone();
        assert_eq!(window.phase(), WindowPhase::Resolving(upper.id()));
        let StackKind::CardSpell(upper_card) = upper.kind else {
            panic!("expected card spell");
        };
        assert_eq!(
            window.cards().get(upper_card).unwrap().zone(),
            ObjectZone::Stack
        );
        let window = window
            .finish_resolution(
                window.revision(),
                upper.id(),
                Some(ObjectDestination::Graveyard),
            )
            .unwrap();
        assert_eq!(
            window.phase(),
            WindowPhase::Priority(PriorityPhase::RequiresChecks { candidate: A })
        );
        assert_eq!(
            window.pass(window.revision(), A),
            Err(StackWindowError::Priority(PriorityError::ChecksRequired))
        );
        assert_eq!(window.entries(), std::slice::from_ref(&lower));
        let b = window
            .cards()
            .get(window.cards().current_identity(CardId(20)).unwrap())
            .unwrap();
        assert_eq!(b.zone(), ObjectZone::Graveyard(B));
        assert_eq!(b.controller(), None);
        let window = passes(window, &[A, B, C]);
        assert_eq!(pending(&window), lower.id());
        let window = window
            .finish_resolution(
                window.revision(),
                lower.id(),
                Some(ObjectDestination::Battlefield { controller: C }),
            )
            .unwrap();
        let a = window
            .cards()
            .get(window.cards().current_identity(CardId(10)).unwrap())
            .unwrap();
        assert_eq!(a.owner(), A);
        assert_eq!(a.controller(), Some(C));
        assert!(window.entries().is_empty());
        assert_eq!(passes(window, &[A, B, C]).phase(), WindowPhase::StepEnded);
    }
    #[test]
    fn an_ability_retains_its_source_snapshot_after_another_spell_moves_the_source() {
        let a = GameObjects::new(0)
            .seed_card(
                0,
                CardId(10),
                A,
                ObjectDestination::Battlefield { controller: A },
            )
            .unwrap();
        let source = a.after.clone();
        let b = a
            .next
            .seed_card(1, CardId(20), B, ObjectDestination::Hand)
            .unwrap();
        let window = ready(StackWindow::new(vec![A, B, C], A, b.next).unwrap());
        let window = window
            .record_activation(window.revision(), A, source.clone())
            .unwrap();
        let ability = window.entries()[0].clone();
        let window = ready(passes(window, &[A]));
        let window = window
            .record_card_spell(window.revision(), B, b.after.id())
            .unwrap();
        let window = passes(window, &[B, C, A]);
        let response = pending(&window);
        let window = window
            .record_resolution_move(window.revision(), source.id(), ObjectDestination::Graveyard)
            .unwrap();
        assert!(window.cards().get(source.id()).is_none());
        assert_eq!(
            window.entries()[0].kind(),
            &StackKind::ActivatedAbility {
                source: source.clone()
            }
        );
        let window = window
            .finish_resolution(
                window.revision(),
                response,
                Some(ObjectDestination::Graveyard),
            )
            .unwrap();
        let window = passes(window, &[A, B, C]);
        assert_eq!(pending(&window), ability.id());
        let window = window
            .finish_resolution(window.revision(), ability.id(), None)
            .unwrap();
        assert!(window.entries().is_empty());
        assert_eq!(
            window
                .cards()
                .get(window.cards().current_identity(CardId(10)).unwrap())
                .unwrap()
                .zone(),
            ObjectZone::Graveyard(A)
        );
    }
    #[test]
    fn resolution_continues_after_its_spell_leaves_and_never_moves_a_returned_incarnation() {
        let (window, a, b) = setup();
        let window = ready(window);
        let window = window.record_card_spell(window.revision(), A, a).unwrap();
        let window = passes(window, &[A, B, C]);
        let entry = pending(&window);
        let StackKind::CardSpell(spell) = *window.resolving().unwrap().kind() else {
            panic!("expected spell");
        };
        let window = window
            .record_resolution_move(window.revision(), spell, ObjectDestination::Exile)
            .unwrap();
        assert!(window.entries().is_empty());
        assert_eq!(window.phase(), WindowPhase::Resolving(entry));
        let exiled = window.cards().current_identity(CardId(10)).unwrap();
        let window = window
            .record_resolution_move(
                window.revision(),
                exiled,
                ObjectDestination::Battlefield { controller: A },
            )
            .unwrap();
        let returned = window.cards().current_identity(CardId(10)).unwrap();
        let window = window
            .record_resolution_move(window.revision(), b, ObjectDestination::Graveyard)
            .unwrap();
        let saved = window.clone();
        assert_eq!(
            window.finish_resolution(window.revision(), entry, Some(ObjectDestination::Graveyard)),
            Err(StackWindowError::UnexpectedSpellExit)
        );
        assert_eq!(window, saved);
        let window = window
            .finish_resolution(window.revision(), entry, None)
            .unwrap();
        assert_eq!(window.cards().current_identity(CardId(10)), Some(returned));
        assert_eq!(
            window.cards().get(returned).unwrap().zone(),
            ObjectZone::Battlefield
        );
        assert_eq!(
            window.phase(),
            WindowPhase::Priority(PriorityPhase::RequiresChecks { candidate: A })
        );
    }
    #[test]
    fn resolution_can_remove_a_lower_card_spell_without_resolving_it() {
        let window = stacked_two();
        let top = pending(&window);
        let StackKind::CardSpell(lower) = *window.entries()[0].kind() else {
            panic!("expected spell");
        };
        let window = window
            .record_resolution_move(window.revision(), lower, ObjectDestination::Graveyard)
            .unwrap();
        assert_eq!(window.entries().len(), 1);
        assert_eq!(pending(&window), top);
        let window = window
            .finish_resolution(window.revision(), top, Some(ObjectDestination::Graveyard))
            .unwrap();
        assert_eq!(passes(window, &[A, B, C]).phase(), WindowPhase::StepEnded);
    }
    #[test]
    fn actual_trigger_receipt_preserves_pairing_apnap_and_the_original_priority_candidate() {
        let (seeded, object, _) = setup();
        let source = seeded.cards().get(object).unwrap().clone();
        let initial = StackWindow::new(vec![C, B, A], B, seeded.cards().clone()).unwrap();
        assert_eq!(
            initial.apnap_players().unwrap().collect::<Vec<_>>(),
            vec![B, A, C]
        );
        let ready = initial.finish_checks(initial.revision()).unwrap();
        let checking = ready.pass(ready.revision(), B).unwrap();
        assert_eq!(
            checking.phase(),
            WindowPhase::Priority(PriorityPhase::RequiresChecks { candidate: A })
        );
        let accepted = checking.clone();
        let (next, entries) = checking
            .record_ordered_triggers_with_receipt(
                checking.revision(),
                vec![
                    OrderedTrigger {
                        controller: C,
                        source: source.clone(),
                    },
                    OrderedTrigger {
                        controller: B,
                        source: source.clone(),
                    },
                ],
            )
            .unwrap()
            .into_parts();
        assert_eq!(checking, accepted);
        assert_eq!(next.revision(), checking.revision() + 1);
        assert_eq!(next.cards(), checking.cards());
        assert_eq!(entries.len(), 2);
        assert_eq!(
            entries,
            next.entries()
                .iter()
                .map(StackEntry::id)
                .collect::<Vec<_>>()
        );
        for (entry, controller) in next.entries().iter().zip([C, B]) {
            assert_eq!(entry.controller(), controller);
            assert_eq!(
                entry.kind(),
                &StackKind::TriggeredAbility {
                    source: source.clone()
                }
            );
        }
        assert_eq!(next.phase(), checking.phase());
    }
    #[test]
    fn preordered_triggers_preserve_the_priority_candidate_and_require_a_fresh_pass_cycle() {
        let (window, a, _) = setup();
        let source = window.cards().get(a).unwrap().clone();
        let window = ready(window);
        let window = window.record_card_spell(window.revision(), A, a).unwrap();
        let window = passes(window, &[A]);
        let window = window
            .record_ordered_triggers(
                window.revision(),
                vec![
                    OrderedTrigger {
                        controller: A,
                        source: source.clone(),
                    },
                    OrderedTrigger {
                        controller: C,
                        source,
                    },
                ],
            )
            .unwrap();
        assert_eq!(
            window
                .entries()
                .iter()
                .map(StackEntry::controller)
                .collect::<Vec<_>>(),
            vec![A, A, C]
        );
        assert_eq!(
            window.phase(),
            WindowPhase::Priority(PriorityPhase::RequiresChecks { candidate: B })
        );
        let window = passes(window, &[B, C]);
        assert_eq!(
            window.phase(),
            WindowPhase::Priority(PriorityPhase::RequiresChecks { candidate: A })
        );
        let window = passes(window, &[A]);
        assert_eq!(window.resolving().unwrap().controller(), C);
        let window = window
            .finish_resolution(window.revision(), pending(&window), None)
            .unwrap();
        assert_eq!(
            window.phase(),
            WindowPhase::Priority(PriorityPhase::RequiresChecks { candidate: A })
        );
    }
    #[test]
    fn activation_accepts_a_source_snapshot_that_precedes_a_source_moving_cost() {
        let source = GameObjects::new(0)
            .seed_card(
                0,
                CardId(10),
                A,
                ObjectDestination::Battlefield { controller: A },
            )
            .unwrap();
        let moved = source
            .next
            .apply_zone_move(1, source.after.id(), ObjectDestination::Graveyard)
            .unwrap()
            .require_moved()
            .unwrap();
        let window = ready(StackWindow::new(vec![A, B], A, moved.next).unwrap());
        let window = window
            .record_activation(window.revision(), A, source.after.clone())
            .unwrap();
        assert_eq!(
            window.entries()[0].kind(),
            &StackKind::ActivatedAbility {
                source: source.after
            }
        );
        let window = passes(window, &[A, B]);
        let window = window
            .finish_resolution(window.revision(), pending(&window), None)
            .unwrap();
        assert_eq!(
            window.cards().get(moved.after.id()).unwrap().zone(),
            ObjectZone::Graveyard(A)
        );
    }
    #[test]
    fn wrong_phase_stale_actor_and_completion_inputs_preserve_the_window() {
        let (initial, a, b) = setup();
        let saved = initial.clone();
        assert_eq!(
            initial.record_card_spell(initial.revision(), A, a),
            Err(StackWindowError::Priority(PriorityError::ChecksRequired))
        );
        assert_eq!(
            initial.record_ordered_triggers(initial.revision(), vec![]),
            Err(StackWindowError::EmptyTriggerBatch)
        );
        assert_eq!(
            initial.record_resolution_move(initial.revision(), a, ObjectDestination::Exile),
            Err(StackWindowError::NotResolving)
        );
        assert_eq!(initial, saved);
        let window = ready(initial);
        let saved = window.clone();
        let source = window.cards().get(a).unwrap().clone();
        assert_eq!(
            window.record_card_spell(0, A, a),
            Err(StackWindowError::StaleRevision)
        );
        assert_eq!(
            window.record_card_spell(window.revision(), B, b),
            Err(StackWindowError::Priority(PriorityError::WrongPlayer))
        );
        assert_eq!(
            window.record_ordered_triggers(
                window.revision(),
                vec![OrderedTrigger {
                    controller: A,
                    source: source.clone()
                }]
            ),
            Err(StackWindowError::ChecksNotPending)
        );
        assert_eq!(window, saved);
        let window = window.record_card_spell(window.revision(), A, a).unwrap();
        let window = passes(window, &[A, B, C]);
        let saved = window.clone();
        let entry = pending(&window);
        assert_eq!(
            window.finish_checks(window.revision()),
            Err(StackWindowError::NotInPriority)
        );
        assert_eq!(
            window.pass(window.revision(), A),
            Err(StackWindowError::NotInPriority)
        );
        assert_eq!(
            window.record_activation(window.revision(), A, source),
            Err(StackWindowError::NotInPriority)
        );
        assert_eq!(
            window.record_card_spell(window.revision(), B, b),
            Err(StackWindowError::NotInPriority)
        );
        assert_eq!(
            window.finish_resolution(window.revision(), StackEntryId(999), None),
            Err(StackWindowError::WrongResolution)
        );
        assert_eq!(
            window.finish_resolution(window.revision(), entry, None),
            Err(StackWindowError::MissingSpellExit)
        );
        assert_eq!(
            window.finish_resolution(
                window.revision(),
                entry,
                Some(ObjectDestination::Battlefield {
                    controller: SeatId(99)
                })
            ),
            Err(StackWindowError::UnknownPlayer)
        );
        assert_eq!(
            window.record_resolution_move(
                window.revision(),
                b,
                ObjectDestination::Stack { controller: B }
            ),
            Err(StackWindowError::UnsupportedStackInsertion)
        );
        assert_eq!(window, saved);
    }
    #[test]
    fn failed_allocations_never_commit_a_card_move_or_partial_trigger_batch() {
        let (window, a, _) = setup();
        let source = window.cards().get(a).unwrap().clone();
        let mut window = ready(window);
        window.next_entry = u64::MAX;
        let saved = window.clone();
        assert_eq!(
            window.record_card_spell(window.revision(), A, a),
            Err(StackWindowError::EntryIdentityExhausted)
        );
        assert_eq!(window, saved);
        assert_eq!(window.cards().get(a).unwrap().zone(), ObjectZone::Hand(A));
        let mut window = StackWindow::new(vec![A, B], A, GameObjects::new(0)).unwrap();
        window.next_entry = u64::MAX - 1;
        let saved = window.clone();
        assert_eq!(
            window.record_ordered_triggers(
                0,
                vec![
                    OrderedTrigger {
                        controller: A,
                        source: source.clone()
                    },
                    OrderedTrigger {
                        controller: B,
                        source: source.clone()
                    }
                ]
            ),
            Err(StackWindowError::EntryIdentityExhausted)
        );
        assert_eq!(window, saved);
        let one = window
            .record_ordered_triggers(
                0,
                vec![OrderedTrigger {
                    controller: A,
                    source,
                }],
            )
            .unwrap();
        assert_eq!(one.entries()[0].id(), StackEntryId(u64::MAX - 1));
        let mut exhausted = window.clone();
        exhausted.revision = u64::MAX;
        assert_eq!(
            exhausted.finish_checks(u64::MAX),
            Err(StackWindowError::RevisionExhausted)
        );
        let card = GameObjects::new(u64::MAX - 1)
            .seed_card(u64::MAX - 1, CardId(1), A, ObjectDestination::Hand)
            .unwrap();
        let window = ready(StackWindow::new(vec![A, B], A, card.next).unwrap());
        let saved = window.clone();
        assert_eq!(
            window.record_card_spell(window.revision(), A, card.after.id()),
            Err(StackWindowError::Objects(
                ObjectIdentityError::RevisionExhausted
            ))
        );
        assert_eq!(window, saved);
    }
    #[test]
    fn setup_rejects_unknown_seats_or_untracked_stack_objects() {
        for destination in [
            ObjectDestination::Hand,
            ObjectDestination::Battlefield {
                controller: SeatId(99),
            },
        ] {
            let owner = if destination == ObjectDestination::Hand {
                SeatId(99)
            } else {
                A
            };
            let card = GameObjects::new(0)
                .seed_card(0, CardId(1), owner, destination)
                .unwrap();
            assert_eq!(
                StackWindow::new(vec![A, B], A, card.next),
                Err(StackWindowError::UnknownPlayer)
            );
        }
        let card = GameObjects::new(0)
            .seed_card(0, CardId(1), A, ObjectDestination::Stack { controller: A })
            .unwrap();
        assert_eq!(
            StackWindow::new(vec![A, B], A, card.next),
            Err(StackWindowError::UntrackedInitialStack)
        );
        assert_eq!(
            StackWindow::new(vec![A, A], A, GameObjects::new(0)),
            Err(StackWindowError::Priority(PriorityError::InvalidOrder))
        );
        assert_eq!(
            StackWindow::new(vec![A, B], C, GameObjects::new(0)),
            Err(StackWindowError::Priority(
                PriorityError::UnknownFirstPlayer
            ))
        );
    }
    #[test]
    fn ability_inputs_reject_foreign_seats_without_partial_updates() {
        let (checking, _, _) = setup();
        let holding = ready(checking.clone());
        let unknown_owner = GameObjects::new(0)
            .seed_card(0, CardId(99), SeatId(99), ObjectDestination::Hand)
            .unwrap()
            .after;
        let unknown_controller = GameObjects::new(0)
            .seed_card(
                0,
                CardId(99),
                A,
                ObjectDestination::Battlefield {
                    controller: SeatId(99),
                },
            )
            .unwrap()
            .after;
        for source in [unknown_owner, unknown_controller] {
            assert_eq!(
                holding.record_activation(holding.revision(), A, source.clone()),
                Err(StackWindowError::UnknownPlayer)
            );
            assert_eq!(
                checking.record_ordered_triggers(
                    checking.revision(),
                    vec![OrderedTrigger {
                        controller: A,
                        source
                    }]
                ),
                Err(StackWindowError::UnknownPlayer)
            );
        }
        let source = checking.cards().iter().next().unwrap().clone();
        assert_eq!(
            checking.record_ordered_triggers(
                checking.revision(),
                vec![OrderedTrigger {
                    controller: SeatId(99),
                    source
                }]
            ),
            Err(StackWindowError::UnknownPlayer)
        );
        assert!(checking.entries().is_empty());
        assert!(holding.entries().is_empty());
        assert_eq!(checking.revision(), 0);
        assert_eq!(holding.revision(), 1);
    }
    #[test]
    fn an_old_resolution_id_cannot_complete_a_later_ability() {
        let (window, a, _) = setup();
        let source = window.cards().get(a).unwrap().clone();
        let window = ready(window);
        let window = window
            .record_activation(window.revision(), A, source.clone())
            .unwrap();
        let window = passes(window, &[A, B, C]);
        let old = pending(&window);
        assert_eq!(
            window.finish_resolution(window.revision(), old, Some(ObjectDestination::Graveyard)),
            Err(StackWindowError::UnexpectedSpellExit)
        );
        let window = ready(
            window
                .finish_resolution(window.revision(), old, None)
                .unwrap(),
        );
        let window = window
            .record_activation(window.revision(), A, source)
            .unwrap();
        let window = passes(window, &[A, B, C]);
        assert_ne!(pending(&window), old);
        let saved = window.clone();
        assert_eq!(
            window.finish_resolution(window.revision(), old, None),
            Err(StackWindowError::WrongResolution)
        );
        assert_eq!(window, saved);
    }
    #[test]
    fn a_spell_cannot_capture_itself_as_a_stack_target() {
        let (window, card, _) = setup();
        let window = ready(window);
        let saved = window.clone();
        let error = window.record_targeted_card_spell(
            window.revision(),
            A,
            card,
            vec![TargetClause {
                id: crate::targets::TargetClauseId(1),
                minimum: 1,
                maximum: 1,
            }],
            vec![TargetChoice {
                id: TargetId(1),
                clause: crate::targets::TargetClauseId(1),
                target: TargetRef::Stack(StackEntryId(1)),
            }],
        );
        assert_eq!(
            error,
            Err(StackWindowError::Targets(TargetError::SelfTarget))
        );
        assert_eq!(window, saved);
    }

    fn unbound_placement() -> (StackWindow, StackEntryId) {
        let (window, object, _) = setup();
        let source = window.cards().get(object).unwrap().clone();
        let (window, entries) = window
            .record_ordered_triggers_with_receipt(
                window.revision(),
                vec![OrderedTrigger {
                    controller: A,
                    source,
                }],
            )
            .unwrap()
            .into_parts();
        (window, entries[0])
    }
    fn single_clause() -> Vec<TargetClause> {
        vec![TargetClause {
            id: crate::targets::TargetClauseId(0),
            minimum: 1,
            maximum: 1,
        }]
    }
    fn single_choice(target: TargetRef) -> Vec<TargetChoice> {
        vec![TargetChoice {
            id: crate::TargetId(0),
            clause: crate::targets::TargetClauseId(0),
            target,
        }]
    }
    #[test]
    fn trigger_bind_and_remove_preserve_checks_and_never_reuse_the_removed_identity() {
        let (window, entry) = unbound_placement();
        let saved = window.clone();
        let bound = window
            .bind_trigger_target(
                window.revision(),
                entry,
                single_clause(),
                single_choice(TargetRef::Player(B)),
            )
            .unwrap();
        assert_eq!(bound.phase(), window.phase());
        assert_eq!(
            bound.entries().last().unwrap().targets().targets()[0]
                .choice()
                .target,
            TargetRef::Player(B)
        );
        assert_eq!(
            bound.remove_unbound_trigger(bound.revision(), entry),
            Err(StackWindowError::InconsistentStack)
        );
        let removed = window
            .remove_unbound_trigger(window.revision(), entry)
            .unwrap();
        assert!(removed.entries().is_empty());
        assert_eq!(removed.phase(), window.phase());
        assert_eq!(removed.next_entry, window.next_entry);
        let object = removed.cards().iter().next().unwrap().clone();
        let (_, entries) = removed
            .record_ordered_triggers_with_receipt(
                removed.revision(),
                vec![OrderedTrigger {
                    controller: A,
                    source: object,
                }],
            )
            .unwrap()
            .into_parts();
        assert_ne!(entries[0], entry);
        assert_eq!(window, saved);
    }
    #[test]
    fn trigger_bind_rejects_nonmandatory_and_malformed_targets_atomically() {
        let (window, entry) = unbound_placement();
        let saved = window.clone();
        for clauses in [
            vec![],
            vec![TargetClause {
                id: crate::targets::TargetClauseId(0),
                minimum: 0,
                maximum: 1,
            }],
            vec![TargetClause {
                id: crate::targets::TargetClauseId(0),
                minimum: 1,
                maximum: 2,
            }],
        ] {
            assert_eq!(
                window.bind_trigger_target(
                    window.revision(),
                    entry,
                    clauses,
                    single_choice(TargetRef::Player(B))
                ),
                Err(StackWindowError::Targets(TargetError::InvalidClause))
            );
            assert_eq!(window, saved);
        }
        assert_eq!(
            window.bind_trigger_target(window.revision(), entry, single_clause(), vec![]),
            Err(StackWindowError::Targets(TargetError::TargetCount))
        );
        for target in [
            TargetRef::Player(SeatId(99)),
            TargetRef::Stack(StackEntryId(999)),
            TargetRef::Stack(entry),
        ] {
            assert!(matches!(
                window.bind_trigger_target(
                    window.revision(),
                    entry,
                    single_clause(),
                    single_choice(target)
                ),
                Err(StackWindowError::Targets(_))
            ));
            assert_eq!(window, saved);
        }
        let mut choices = single_choice(TargetRef::Player(B));
        choices[0].clause = crate::targets::TargetClauseId(1);
        assert_eq!(
            window.bind_trigger_target(window.revision(), entry, single_clause(), choices),
            Err(StackWindowError::Targets(TargetError::UnknownClause))
        );
        assert_eq!(window, saved);
    }
    #[test]
    fn both_trigger_operations_share_phase_entry_and_operational_guards() {
        let (window, entry) = unbound_placement();
        let ready_window = window.finish_checks(window.revision()).unwrap();
        let source = window.cards().iter().next().unwrap().clone();
        let not_top = window
            .record_ordered_triggers(
                window.revision(),
                vec![OrderedTrigger {
                    controller: A,
                    source,
                }],
            )
            .unwrap();
        let mut exhausted = window.clone();
        exhausted.revision = u64::MAX;
        for (state, id, expected, error) in [
            (
                &ready_window,
                entry,
                ready_window.revision(),
                StackWindowError::ChecksNotPending,
            ),
            (
                &not_top,
                entry,
                not_top.revision(),
                StackWindowError::InconsistentStack,
            ),
            (
                &window,
                StackEntryId(999),
                window.revision(),
                StackWindowError::InconsistentStack,
            ),
            (
                &window,
                entry,
                window.revision() - 1,
                StackWindowError::StaleRevision,
            ),
            (
                &exhausted,
                entry,
                u64::MAX,
                StackWindowError::RevisionExhausted,
            ),
        ] {
            let saved = state.clone();
            assert_eq!(
                state.bind_trigger_target(
                    expected,
                    id,
                    single_clause(),
                    single_choice(TargetRef::Player(B))
                ),
                Err(error)
            );
            assert_eq!(state.remove_unbound_trigger(expected, id), Err(error));
            assert_eq!(state, &saved);
        }
        let (initial, card, _) = setup();
        let ready = ready(initial);
        let spell = ready.record_card_spell(ready.revision(), A, card).unwrap();
        let id = spell.entries().last().unwrap().id();
        assert_eq!(
            spell.bind_trigger_target(
                spell.revision(),
                id,
                single_clause(),
                single_choice(TargetRef::Player(B))
            ),
            Err(StackWindowError::InconsistentStack)
        );
        assert_eq!(
            spell.remove_unbound_trigger(spell.revision(), id),
            Err(StackWindowError::InconsistentStack)
        );
    }

    #[test]
    fn trigger_plan_never_follows_a_returned_card_to_its_new_incarnation() {
        // Trusted identity/TargetPlan probe, not a public blink trajectory or
        // an EffectWindow characteristic/source-publication qualification.
        let (mut window, _, _) = setup();
        let seeded = window
            .cards
            .seed_card(
                window.cards.revision(),
                CardId(30),
                A,
                ObjectDestination::Battlefield { controller: A },
            )
            .unwrap();
        let old = seeded.after.id();
        window.cards = seeded.next;
        let source = window.cards.get(old).unwrap().clone();
        let (mut window, entries) = window
            .record_ordered_triggers_with_receipt(
                window.revision(),
                vec![OrderedTrigger {
                    controller: A,
                    source,
                }],
            )
            .unwrap()
            .into_parts();
        let entry = entries[0];
        window = window
            .bind_trigger_target(
                window.revision(),
                entry,
                single_clause(),
                single_choice(TargetRef::Object(old)),
            )
            .unwrap();
        let moved = window
            .cards
            .apply_zone_move(window.cards.revision(), old, ObjectDestination::Hand)
            .unwrap()
            .require_moved()
            .unwrap();
        let hand = moved.after.id();
        window.cards = moved.next;
        let returned = window
            .cards
            .apply_zone_move(
                window.cards.revision(),
                hand,
                ObjectDestination::Battlefield { controller: A },
            )
            .unwrap()
            .require_moved()
            .unwrap();
        let current = returned.after.id();
        window.cards = returned.next;
        assert_ne!(old, current);
        assert!(window.cards.get(old).is_none());
        let pending = passes(ready(window), &[A, B, C]);
        let checked = pending
            .check_resolution_targets(
                pending.revision(),
                entry,
                vec![TargetCheck {
                    target: crate::TargetId(0),
                    legal: true,
                }],
            )
            .unwrap();
        assert_eq!(checked.phase(), WindowPhase::AllTargetsIllegal(entry));
        assert_eq!(
            checked
                .target_assessment()
                .unwrap()
                .get(crate::TargetId(0))
                .unwrap()
                .legality,
            crate::targets::TargetLegality::ObjectGone
        );
        assert_eq!(checked.cards.current_identity(CardId(30)), Some(current));
    }
}
