//! Native individual-turn stack/priority boundary (CR 117, 405, 608).
//!
//! These are trusted post-legality continuations, not player commands. No card
//! program, costs, complete target-legality predicates, state-based actions or trigger discovery/APNAP ordering,
//! turn progression or persistence is implemented here. Identities are window-local;
//! the eventual host scopes them to a game and serializes accepted successors.
use crate::{
    TargetId,
    card_identity::{
        CardDestination, CardIdentityError, CardObject, CardObjectId, CardObjects, CardZone,
    },
    priority::{PriorityError, PriorityPhase, PriorityRound, SeatId},
    targets::{
        ResolutionPermission, TargetAssessment, TargetCheck, TargetChoice, TargetClause,
        TargetError, TargetLegality, TargetPlan, TargetRef, TargetWorld,
    },
};

use std::collections::BTreeMap;

/// One stack entry, including abilities that have no associated physical card.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct StackEntryId(u64);

/// Minimal source evidence only, not a complete ability program or last-known state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StackKind {
    CardSpell(CardObjectId),
    /// A nonpermanent instant copy has no physical CardId. Payload is owned by the executor.
    SpellCopy {
        copied_from: StackEntryId,
        owner: SeatId,
    },
    ActivatedAbility {
        source: CardObject,
    },
    TriggeredAbility {
        source: CardObject,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StackEntry {
    id: StackEntryId,
    controller: SeatId,
    kind: StackKind,
    targets: TargetPlan,
}
impl StackEntry {
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

/// A transient non-card spell object. Its key tracks one copy's lifetime, not a
/// physical card or a targetable object outside the stack. Permanent copies and
/// replacement-adjusted destinations are unsupported in this narrow slice.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpellCopyObject {
    pub id: StackEntryId,
    pub owner: SeatId,
    pub controller: Option<SeatId>,
    pub zone: CardZone,
}
/// A nonmana, stack-using trigger whose controller/source and APNAP position the kernel has
/// already established. Vector order is bottom-to-top, never chosen by this module.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OrderedTrigger {
    pub controller: SeatId,
    pub source: CardObject,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowPhase {
    Priority(PriorityPhase),
    Resolving(StackEntryId),
    CheckingTargets(StackEntryId),
    AllTargetsIllegal(StackEntryId),
    StepEnded,
}
#[derive(Debug, Clone, PartialEq, Eq)]
enum Status {
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
    active: SeatId,
    cards: CardObjects,
    stack: Vec<StackEntry>,
    copies: BTreeMap<StackEntryId, SpellCopyObject>,
    status: Status,
}

/// Native rejection or unsupported integration, not an adjudicated game outcome.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StackWindowError {
    StaleRevision,
    RevisionExhausted,
    EntryIdentityExhausted,
    WrongController,
    SourceTapped,
    UnknownPlayer,
    UntrackedInitialStack,
    Priority(PriorityError),
    Cards(CardIdentityError),
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
    NotSpell,
    CopyChecksRequired,
}
impl From<PriorityError> for StackWindowError {
    fn from(error: PriorityError) -> Self {
        Self::Priority(error)
    }
}
impl From<CardIdentityError> for StackWindowError {
    fn from(error: CardIdentityError) -> Self {
        Self::Cards(error)
    }
}

impl From<TargetError> for StackWindowError {
    fn from(error: TargetError) -> Self {
        Self::Targets(error)
    }
}

impl TargetWorld for StackWindow {
    fn card_zone(&self, card: CardObjectId) -> Option<CardZone> {
        self.cards.get(card).map(CardObject::zone)
    }
    fn player_exists(&self, player: SeatId) -> bool {
        self.order.contains(&player)
    }
    fn stack_exists(&self, entry: StackEntryId) -> bool {
        self.stack.iter().any(|item| item.id == entry)
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
    /// Begin at a caller-selected step boundary after turn-based work. No player
    /// gets priority until the enclosing kernel settles CR 117.5 and finishes checks.
    /// Existing stack objects need a future explicit restoration contract, not inference.
    pub fn new(
        order: Vec<SeatId>,
        active: SeatId,
        cards: CardObjects,
    ) -> Result<Self, StackWindowError> {
        let round = PriorityRound::new(order.clone(), active, 0)?;
        for card in cards.iter() {
            if !order.contains(&card.owner())
                || card.controller().is_some_and(|seat| !order.contains(&seat))
            {
                return Err(StackWindowError::UnknownPlayer);
            }
            if card.zone() == CardZone::Stack {
                return Err(StackWindowError::UntrackedInitialStack);
            }
        }
        Ok(Self {
            revision: 0,
            next_entry: 1,
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
    pub fn cards(&self) -> &CardObjects {
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
            StackKind::CardSpell(object) => self.cards.get(*object).map(CardObject::owner),
            StackKind::SpellCopy { owner, .. } => Some(*owner),
            _ => None,
        }
    }
    pub fn phase(&self) -> WindowPhase {
        match &self.status {
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
    fn priority(&self) -> Result<&PriorityRound, StackWindowError> {
        match &self.status {
            Status::Priority(round) => Ok(round),
            _ => Err(StackWindowError::NotInPriority),
        }
    }
    fn validate_player(&self, seat: SeatId) -> Result<(), StackWindowError> {
        if self.order.contains(&seat) {
            Ok(())
        } else {
            Err(StackWindowError::UnknownPlayer)
        }
    }
    fn validate_source(&self, source: &CardObject) -> Result<(), StackWindowError> {
        self.validate_player(source.owner())?;
        if let Some(controller) = source.controller() {
            self.validate_player(controller)?;
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
            .any(|copy| copy.zone != CardZone::Stack)
        {
            return Err(StackWindowError::CopyChecksRequired);
        }
        let round = round.finish_checks(round.revision())?;
        Ok(Self {
            revision,
            status: Status::Priority(round),
            ..self.clone()
        })
    }
    pub fn pass(&self, expected: u64, player: SeatId) -> Result<Self, StackWindowError> {
        let revision = self.next_revision(expected)?;
        let round = self.priority()?;
        let round = round.pass(round.revision(), player)?;
        let status = if round.phase() == PriorityPhase::AllPassed {
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
        };
        Ok(Self {
            revision,
            status,
            ..self.clone()
        })
    }
    /// Trusted completed tap activation. Reuse priority action/check bookkeeping,
    /// but do not allocate or insert a stack entry for a mana ability (CR 605.3b).
    pub fn record_tap_action(
        &self,
        expected: u64,
        actor: SeatId,
        source: CardObjectId,
    ) -> Result<Self, StackWindowError> {
        let revision = self.next_revision(expected)?;
        let round = self.priority()?;
        let round = round.record_action(round.revision(), actor)?;
        let card = self
            .cards
            .get(source)
            .ok_or(CardIdentityError::UnknownObject)?;
        if card.zone() != CardZone::Battlefield {
            return Err(CardIdentityError::NotPermanent.into());
        }
        if card.controller() != Some(actor) {
            return Err(StackWindowError::WrongController);
        }
        if card.tapped() != Some(false) {
            return Err(StackWindowError::SourceTapped);
        }
        let changed = self.cards.set_tapped(self.cards.revision(), source, true)?;
        Ok(Self {
            revision,
            cards: changed.next,
            status: Status::Priority(round),
            ..self.clone()
        })
    }
    #[cfg(test)]
    pub(crate) fn set_test_card_revision(&mut self, revision: u64) {
        self.cards.set_test_revision(revision);
    }
    #[cfg(test)]
    pub(crate) fn set_test_priority_revision(&mut self, revision: u64) {
        let Status::Priority(round) = &mut self.status else {
            panic!("priority fault injection requires a priority window");
        };
        round.set_test_revision(revision);
    }
    /// Record an otherwise legal card cast with a new stack incarnation. This narrow
    /// move/push/reset is atomic in the returned native successor; it does not pay
    /// costs or implement the intervening casting procedure (CR 601).
    pub fn record_card_spell(
        &self,
        expected: u64,
        actor: SeatId,
        card: CardObjectId,
    ) -> Result<Self, StackWindowError> {
        let revision = self.next_revision(expected)?;
        let round = self.priority()?;
        let round = round.record_action(round.revision(), actor)?;
        let moved = self.cards.apply_zone_move(
            self.cards.revision(),
            card,
            CardDestination::Stack { controller: actor },
        )?;
        let mut next = self.clone();
        let entry = next.allocate(actor, StackKind::CardSpell(moved.after.id()))?;
        next.stack.push(entry);
        next.cards = moved.next;
        next.status = Status::Priority(round);
        next.revision = revision;
        Ok(next)
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
        let revision = self.next_revision(expected)?;
        if !matches!(self.status, Status::Resolving { .. }) {
            return Err(StackWindowError::NotResolving);
        }
        self.validate_player(controller)?;
        let source = self
            .stack
            .iter()
            .find(|entry| entry.id == original)
            .ok_or(StackWindowError::NotSpell)?;
        if !matches!(
            source.kind,
            StackKind::CardSpell(_) | StackKind::SpellCopy { .. }
        ) {
            return Err(StackWindowError::NotSpell);
        }
        let targets = source.targets.clone();
        let mut next = self.clone();
        let mut entry = next.allocate(
            controller,
            StackKind::SpellCopy {
                copied_from: original,
                owner: controller,
            },
        )?;
        next.stack.push(entry.clone());
        entry.targets = targets.copy_with_changes(&next, entry.id, changes)?;
        *next
            .stack
            .last_mut()
            .ok_or(StackWindowError::InconsistentStack)? = entry.clone();
        next.copies.insert(
            entry.id,
            SpellCopyObject {
                id: entry.id,
                owner: controller,
                controller: Some(controller),
                zone: CardZone::Stack,
            },
        );
        next.revision = revision;
        Ok(next)
    }
    /// CR 704.5e: exited spell copies cease to exist during pending state checks,
    /// not as an instruction in the middle of resolution.
    pub fn cease_departed_copies(&self, expected: u64) -> Result<Self, StackWindowError> {
        let revision = self.next_revision(expected)?;
        if !matches!(
            self.priority()?.phase(),
            PriorityPhase::RequiresChecks { .. }
        ) {
            return Err(StackWindowError::ChecksNotPending);
        }
        let mut next = self.clone();
        next.copies.retain(|_, copy| copy.zone == CardZone::Stack);
        next.revision = revision;
        Ok(next)
    }
    fn move_copy(
        &self,
        revision: u64,
        id: StackEntryId,
        destination: CardDestination,
    ) -> Result<Self, StackWindowError> {
        if destination != CardDestination::Graveyard {
            return Err(StackWindowError::UnsupportedTargetOperation);
        }
        let mut next = self.clone();
        let copy = next
            .copies
            .get_mut(&id)
            .ok_or(StackWindowError::InconsistentStack)?;
        if copy.zone != CardZone::Stack {
            return Err(StackWindowError::InconsistentStack);
        }
        copy.zone = CardZone::Graveyard(copy.owner);
        copy.controller = None;
        next.stack.retain(|entry| entry.id != id);
        next.revision = revision;
        Ok(next)
    }
    /// Record an otherwise legal nonmana activation. The supplied source snapshot
    /// can precede costs that moved the source; it need not still be current.
    /// Cost execution and full ability/source context are not implemented here.
    pub fn record_activation(
        &self,
        expected: u64,
        actor: SeatId,
        source: CardObject,
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
    /// Apply a preordered nonmana trigger batch during checks. The same candidate remains
    /// pending; the changed stack requires a fresh pass round. No discovery or player
    /// ordering decision is synthesized, and no controller silently gains priority.
    pub fn record_ordered_triggers(
        &self,
        expected: u64,
        triggers: Vec<OrderedTrigger>,
    ) -> Result<Self, StackWindowError> {
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
        for trigger in triggers {
            let entry = next.allocate(
                trigger.controller,
                StackKind::TriggeredAbility {
                    source: trigger.source,
                },
            )?;
            next.stack.push(entry);
        }
        next.status = Status::Priority(PriorityRound::new(self.order.clone(), candidate, 0)?);
        next.revision = revision;
        Ok(next)
    }
    /// One already-adjudicated resolution instruction, not an effect interpreter.
    /// Moving a card spell removes its stack entry, but a resolving continuation
    /// survives its own card's exit (CR 608.2m). Casting another physical card during
    /// resolution remains unsupported; spell-copy creation uses its explicit method.
    pub fn record_resolution_move(
        &self,
        expected: u64,
        card: CardObjectId,
        destination: CardDestination,
    ) -> Result<Self, StackWindowError> {
        let revision = self.next_revision(expected)?;
        if !matches!(self.status, Status::Resolving { .. }) {
            return Err(StackWindowError::NotResolving);
        }
        match destination {
            CardDestination::Stack { .. } => {
                return Err(StackWindowError::UnsupportedStackInsertion);
            }
            CardDestination::Battlefield { controller } => self.validate_player(controller)?,
            _ => {}
        }
        self.move_card(revision, card, destination)
    }
    /// Trusted replacement-adjusted state-based zone moves, only while CR 117.5
    /// checks are pending. No priority is granted and no triggers are discovered.
    /// Each move is applied to the local successor; any rejection rolls back the batch.
    pub fn record_check_moves(
        &self,
        expected: u64,
        moves: Vec<(CardObjectId, CardDestination)>,
    ) -> Result<Self, StackWindowError> {
        let revision = self.next_revision(expected)?;
        let round = self.priority()?;
        if !matches!(round.phase(), PriorityPhase::RequiresChecks { .. }) {
            return Err(StackWindowError::ChecksNotPending);
        }
        if moves.is_empty() {
            return Err(StackWindowError::EmptyCheckMoves);
        }
        let mut next = self.clone();
        for (card, destination) in moves {
            if next.cards.get(card).map(CardObject::zone) != Some(CardZone::Battlefield) {
                return Err(StackWindowError::InvalidCheckMove);
            }
            match destination {
                CardDestination::Stack { .. } | CardDestination::Battlefield { .. } => {
                    return Err(StackWindowError::InvalidCheckMove);
                }
                _ => {}
            }
            next = next.move_card(revision, card, destination)?;
        }
        Ok(next)
    }
    fn move_card(
        &self,
        revision: u64,
        card: CardObjectId,
        destination: CardDestination,
    ) -> Result<Self, StackWindowError> {
        let moved = self
            .cards
            .apply_zone_move(self.cards.revision(), card, destination)?;
        let mut next = self.clone();
        next.cards = moved.next;
        next.revision = revision;
        next.stack
            .retain(|entry| !matches!(entry.kind,StackKind::CardSpell(object) if object==card));
        Ok(next)
    }
    /// Trusted completion after all resolution work/choices have finished. A card
    /// still on the stack requires its computed exit; an already departed card or
    /// an ability requires None. Never follow a stable card key to move a new object.
    /// Active-player priority is still blocked on CR 117.5 checks (CR 117.3b).
    pub fn finish_resolution(
        &self,
        expected: u64,
        entry: StackEntryId,
        exit: Option<CardDestination>,
    ) -> Result<Self, StackWindowError> {
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
        exit: Option<CardDestination>,
    ) -> Result<Self, StackWindowError> {
        if pending.id != entry {
            return Err(StackWindowError::WrongResolution);
        }
        let mut next = self.clone();
        match pending.kind {
            StackKind::CardSpell(object) => {
                if let Some(card) = self.cards.get(object) {
                    if card.zone() != CardZone::Stack {
                        return Err(StackWindowError::InconsistentStack);
                    }
                    let destination = exit.ok_or(StackWindowError::MissingSpellExit)?;
                    match destination {
                        CardDestination::Stack { .. } => {
                            return Err(StackWindowError::UnsupportedStackInsertion);
                        }
                        CardDestination::Battlefield { controller } => {
                            self.validate_player(controller)?
                        }
                        _ => {}
                    }
                    let moved =
                        self.cards
                            .apply_zone_move(self.cards.revision(), object, destination)?;
                    next.cards = moved.next;
                } else if exit.is_some() {
                    return Err(StackWindowError::UnexpectedSpellExit);
                }
            }
            StackKind::SpellCopy { .. } => {
                next = self.move_copy(
                    revision,
                    entry,
                    exit.ok_or(StackWindowError::MissingSpellExit)?,
                )?;
            }
            _ => {
                if exit.is_some() {
                    return Err(StackWindowError::UnexpectedSpellExit);
                }
            }
        }
        next.stack.retain(|item| item.id != entry);
        next.status = Status::Priority(PriorityRound::new(self.order.clone(), self.active, 0)?);
        next.revision = revision;
        Ok(next)
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
    fn bind_targets(
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
    /// Post-legality target capture, not casting or legal-domain discovery. Failure
    /// leaves the original window and its card location unchanged.
    pub fn record_targeted_card_spell(
        &self,
        expected: u64,
        actor: SeatId,
        card: CardObjectId,
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
        source: CardObject,
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
    /// All-illegal entries do not resolve. The enclosing kernel supplies the
    /// replacement-adjusted spell exit (normally owner's graveyard), or None for an
    /// ability. This gate cannot be used to run effects or silently skip checks.
    pub fn finish_unresolved(
        &self,
        expected: u64,
        entry: StackEntryId,
        exit: Option<CardDestination>,
    ) -> Result<Self, StackWindowError> {
        let revision = self.next_revision(expected)?;
        let Status::AllTargetsIllegal { entry: pending, .. } = &self.status else {
            return Err(StackWindowError::TargetsNotRejected);
        };
        self.complete_entry(revision, pending, entry, exit)
    }
    /// Minimal binding-aware card-zone instruction. Illegal target occurrences never
    /// become an untargeted move or redirect to a returned card. Player/ability effect
    /// operations and do-as-much-as-possible interpretation are not implemented here.
    pub fn record_targeted_resolution_move(
        &self,
        expected: u64,
        target: TargetId,
        destination: CardDestination,
    ) -> Result<Self, StackWindowError> {
        self.next_revision(expected)?;
        let Status::Resolving { targets, .. } = &self.status else {
            return Err(StackWindowError::NotResolving);
        };
        let result = targets.get(target).ok_or(StackWindowError::UnknownTarget)?;
        if result.legality != TargetLegality::Legal {
            return Err(StackWindowError::IllegalTarget);
        }
        let card = match result.choice.target {
            TargetRef::Card(card) => card,
            TargetRef::Stack(id) => match self
                .stack
                .iter()
                .find(|entry| entry.id == id)
                .map(|entry| &entry.kind)
            {
                Some(StackKind::CardSpell(card)) => *card,
                Some(StackKind::SpellCopy { .. }) => {
                    let revision = self.next_revision(expected)?;
                    return self.move_copy(revision, id, destination);
                }
                _ => return Err(StackWindowError::UnsupportedTargetOperation),
            },
            TargetRef::Player(_) => return Err(StackWindowError::UnsupportedTargetOperation),
        };
        self.record_resolution_move(expected, card, destination)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::card_identity::CardId;
    const A: SeatId = SeatId(0);
    const B: SeatId = SeatId(1);
    const C: SeatId = SeatId(2);
    fn setup() -> (StackWindow, CardObjectId, CardObjectId) {
        let a = CardObjects::new(0)
            .seed_card(0, CardId(10), A, CardDestination::Hand)
            .unwrap();
        let b = a
            .next
            .seed_card(1, CardId(20), B, CardDestination::Hand)
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
                    CardObjects::new(0),
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
            CardZone::Stack
        );
        let window = window
            .finish_resolution(
                window.revision(),
                upper.id(),
                Some(CardDestination::Graveyard),
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
        assert_eq!(b.zone(), CardZone::Graveyard(B));
        assert_eq!(b.controller(), None);
        let window = passes(window, &[A, B, C]);
        assert_eq!(pending(&window), lower.id());
        let window = window
            .finish_resolution(
                window.revision(),
                lower.id(),
                Some(CardDestination::Battlefield { controller: C }),
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
        let a = CardObjects::new(0)
            .seed_card(
                0,
                CardId(10),
                A,
                CardDestination::Battlefield { controller: A },
            )
            .unwrap();
        let source = a.after.clone();
        let b = a
            .next
            .seed_card(1, CardId(20), B, CardDestination::Hand)
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
            .record_resolution_move(window.revision(), source.id(), CardDestination::Graveyard)
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
                Some(CardDestination::Graveyard),
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
            CardZone::Graveyard(A)
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
            .record_resolution_move(window.revision(), spell, CardDestination::Exile)
            .unwrap();
        assert!(window.entries().is_empty());
        assert_eq!(window.phase(), WindowPhase::Resolving(entry));
        let exiled = window.cards().current_identity(CardId(10)).unwrap();
        let window = window
            .record_resolution_move(
                window.revision(),
                exiled,
                CardDestination::Battlefield { controller: A },
            )
            .unwrap();
        let returned = window.cards().current_identity(CardId(10)).unwrap();
        let window = window
            .record_resolution_move(window.revision(), b, CardDestination::Graveyard)
            .unwrap();
        let saved = window.clone();
        assert_eq!(
            window.finish_resolution(window.revision(), entry, Some(CardDestination::Graveyard)),
            Err(StackWindowError::UnexpectedSpellExit)
        );
        assert_eq!(window, saved);
        let window = window
            .finish_resolution(window.revision(), entry, None)
            .unwrap();
        assert_eq!(window.cards().current_identity(CardId(10)), Some(returned));
        assert_eq!(
            window.cards().get(returned).unwrap().zone(),
            CardZone::Battlefield
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
            .record_resolution_move(window.revision(), lower, CardDestination::Graveyard)
            .unwrap();
        assert_eq!(window.entries().len(), 1);
        assert_eq!(pending(&window), top);
        let window = window
            .finish_resolution(window.revision(), top, Some(CardDestination::Graveyard))
            .unwrap();
        assert_eq!(passes(window, &[A, B, C]).phase(), WindowPhase::StepEnded);
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
        let source = CardObjects::new(0)
            .seed_card(
                0,
                CardId(10),
                A,
                CardDestination::Battlefield { controller: A },
            )
            .unwrap();
        let moved = source
            .next
            .apply_zone_move(1, source.after.id(), CardDestination::Graveyard)
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
            CardZone::Graveyard(A)
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
            initial.record_resolution_move(initial.revision(), a, CardDestination::Exile),
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
                Some(CardDestination::Battlefield {
                    controller: SeatId(99)
                })
            ),
            Err(StackWindowError::UnknownPlayer)
        );
        assert_eq!(
            window.record_resolution_move(
                window.revision(),
                b,
                CardDestination::Stack { controller: B }
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
        assert_eq!(window.cards().get(a).unwrap().zone(), CardZone::Hand(A));
        let mut window = StackWindow::new(vec![A, B], A, CardObjects::new(0)).unwrap();
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
        let card = CardObjects::new(u64::MAX - 1)
            .seed_card(u64::MAX - 1, CardId(1), A, CardDestination::Hand)
            .unwrap();
        let window = ready(StackWindow::new(vec![A, B], A, card.next).unwrap());
        let saved = window.clone();
        assert_eq!(
            window.record_card_spell(window.revision(), A, card.after.id()),
            Err(StackWindowError::Cards(
                CardIdentityError::RevisionExhausted
            ))
        );
        assert_eq!(window, saved);
    }
    #[test]
    fn setup_rejects_unknown_seats_or_untracked_stack_objects() {
        for destination in [
            CardDestination::Hand,
            CardDestination::Battlefield {
                controller: SeatId(99),
            },
        ] {
            let owner = if destination == CardDestination::Hand {
                SeatId(99)
            } else {
                A
            };
            let card = CardObjects::new(0)
                .seed_card(0, CardId(1), owner, destination)
                .unwrap();
            assert_eq!(
                StackWindow::new(vec![A, B], A, card.next),
                Err(StackWindowError::UnknownPlayer)
            );
        }
        let card = CardObjects::new(0)
            .seed_card(0, CardId(1), A, CardDestination::Stack { controller: A })
            .unwrap();
        assert_eq!(
            StackWindow::new(vec![A, B], A, card.next),
            Err(StackWindowError::UntrackedInitialStack)
        );
        assert_eq!(
            StackWindow::new(vec![A, A], A, CardObjects::new(0)),
            Err(StackWindowError::Priority(PriorityError::InvalidOrder))
        );
        assert_eq!(
            StackWindow::new(vec![A, B], C, CardObjects::new(0)),
            Err(StackWindowError::Priority(
                PriorityError::UnknownFirstPlayer
            ))
        );
    }
    #[test]
    fn ability_inputs_reject_foreign_seats_without_partial_updates() {
        let (checking, _, _) = setup();
        let holding = ready(checking.clone());
        let unknown_owner = CardObjects::new(0)
            .seed_card(0, CardId(99), SeatId(99), CardDestination::Hand)
            .unwrap()
            .after;
        let unknown_controller = CardObjects::new(0)
            .seed_card(
                0,
                CardId(99),
                A,
                CardDestination::Battlefield {
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
            window.finish_resolution(window.revision(), old, Some(CardDestination::Graveyard)),
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
}
