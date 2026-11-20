//! Shared card/token incarnation bookkeeping (CR 108, 111 and 400.7).
//!
//! Trusted kernel continuations only: callers first resolve legality, replacements and
//! any information retained by a CR 400.7 exception. This module does not implement
//! those rules, hidden-information projections or planar ownership.
use crate::draw::{DrawEvent, DrawJournal, LibraryDraw};
use crate::priority::SeatId;
use crate::program::{DefinitionError, Program};
use crate::shuffle::{EntropyError, EntropyTape, LibraryShuffle, SHUFFLE_ALGORITHM, ShuffleRecord};
use spellwake_contracts::RulesInteger;
use std::collections::{BTreeMap, BTreeSet};
#[cfg(test)]
mod control_tests;
#[cfg(test)]
mod departure_tests;
#[cfg(test)]
mod token_tests;

/// One card supplied to this game, not a catalogue ID, collection grant or rules target.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct CardId(pub u64);

/// One incarnation of a card or token within one registry/game. The enclosing host binds
/// handles to that game and serializes competing successors. Within the accepted
/// state, a stale incarnation cannot address a returned card.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct ObjectId(u64);

/// Opaque control-history identity within one registry and object incarnation.
/// Its source is the registry's checked revision, not a game turn number.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct ControlEpoch(u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ControlHistory {
    epoch: ControlEpoch,
    turn_start_epoch: Option<ControlEpoch>,
}
impl ControlHistory {
    fn new(revision: u64) -> Self {
        Self {
            epoch: ControlEpoch(revision),
            turn_start_epoch: None,
        }
    }
}

/// Individual players have distinct libraries, hands and graveyards.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ObjectZone {
    Library(SeatId),
    Hand(SeatId),
    Graveyard(SeatId),
    Battlefield,
    Stack,
    Exile,
    Command,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum OrderedZone {
    Library(SeatId),
    Graveyard(SeatId),
}
impl OrderedZone {
    fn from_zone(zone: ObjectZone) -> Option<Self> {
        match zone {
            ObjectZone::Library(owner) => Some(Self::Library(owner)),
            ObjectZone::Graveyard(owner) => Some(Self::Graveyard(owner)),
            _ => None,
        }
    }
    fn inconsistency(self) -> ObjectIdentityError {
        match self {
            Self::Library(_) => ObjectIdentityError::InconsistentLibrary,
            Self::Graveyard(_) => ObjectIdentityError::InconsistentOrderedZone,
        }
    }
}

/// Already-established placement. Owned zones always resolve to this card's owner
/// (CR 400.3); a controller must be supplied for a spell or permanent (CR 108.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ObjectDestination {
    LibraryTop,
    LibraryBottom,
    Hand,
    Graveyard,
    Battlefield { controller: SeatId },
    Stack { controller: SeatId },
    Exile,
    Command,
}
impl ObjectDestination {
    fn placement(self, owner: SeatId) -> (ObjectZone, Option<SeatId>) {
        match self {
            Self::LibraryTop | Self::LibraryBottom => (ObjectZone::Library(owner), None),
            Self::Hand => (ObjectZone::Hand(owner), None),
            Self::Graveyard => (ObjectZone::Graveyard(owner), None),
            Self::Battlefield { controller } => (ObjectZone::Battlefield, Some(controller)),
            Self::Stack { controller } => (ObjectZone::Stack, Some(controller)),
            Self::Exile => (ObjectZone::Exile, None),
            Self::Command => (ObjectZone::Command, None),
        }
    }
}

/// Minimal identity/location snapshot, deliberately not a complete Magic object.
/// Origin survives copying, control changes and zone movement (CR 111.6).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ObjectOrigin {
    Card(CardId),
    Token { definition: Box<Program> },
}

/// One current incarnation with its immutable origin and ordinary location/status.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GameObject {
    id: ObjectId,
    origin: ObjectOrigin,
    owner: SeatId,
    zone: ObjectZone,
    controller: Option<SeatId>,
    default_controller: Option<SeatId>,
    tapped: Option<bool>,
    control_history: Option<ControlHistory>,
}
impl GameObject {
    pub fn id(&self) -> ObjectId {
        self.id
    }
    /// Physical-card provenance, absent for a token even when it copies a card.
    pub fn card_id(&self) -> Option<CardId> {
        match self.origin {
            ObjectOrigin::Card(card) => Some(card),
            ObjectOrigin::Token { .. } => None,
        }
    }
    /// Immutable card provenance or the token's owned baseline definition.
    pub fn origin(&self) -> &ObjectOrigin {
        &self.origin
    }
    /// Whether this incarnation has token origin, independently of copy effects.
    pub fn is_token(&self) -> bool {
        matches!(self.origin, ObjectOrigin::Token { .. })
    }
    /// Baseline before copy effects, from the sole definition belonging to this origin.
    pub(crate) fn base_program<'a>(
        &'a self,
        programs: &'a BTreeMap<CardId, Program>,
    ) -> Option<&'a Program> {
        match &self.origin {
            ObjectOrigin::Card(card) => programs.get(card),
            ObjectOrigin::Token { definition } => Some(definition),
        }
    }
    pub fn owner(&self) -> SeatId {
        self.owner
    }
    pub fn zone(&self) -> ObjectZone {
        self.zone
    }
    pub fn controller(&self) -> Option<SeatId> {
        self.controller
    }
    /// Malformed appearance probe only; no admitted registry transition uses this.
    #[cfg(test)]
    pub(crate) fn qualification_without_controller(&self) -> Self {
        Self {
            controller: None,
            ..self.clone()
        }
    }
    /// Controller before continuous control effects, independently of ownership.
    /// Stack placement retains the original caster for the later CR 110.2b case.
    pub fn default_controller(&self) -> Option<SeatId> {
        self.default_controller
    }
    /// Physical status exists only on the battlefield (CR 110.5).
    pub fn tapped(&self) -> Option<bool> {
        self.tapped
    }
    /// Current battlefield control interval; absent outside the battlefield.
    pub fn control_epoch(&self) -> Option<ControlEpoch> {
        self.control_history.map(|history| history.epoch)
    }
    /// Whether this incarnation has stayed under its current controller since
    /// that player's most-recent recorded turn start. This history fact does not
    /// implement attack legality, haste or tap-symbol ability restrictions.
    pub fn has_continuous_control_since_turn_start(&self) -> Option<bool> {
        self.control_history
            .map(|history| history.turn_start_epoch == Some(history.epoch))
    }
}

/// Immutable native state for current cards/tokens; not a public API or durable commit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GameObjects {
    revision: u64,
    next_identity: u64,
    current: BTreeMap<ObjectId, GameObject>,
    by_card: BTreeMap<CardId, ObjectId>,
    ordered_zones: BTreeMap<OrderedZone, Vec<ObjectId>>,
}

/// Internal evidence for a later authoritative event; never a seat-safe projection.
/// The prior snapshot is retained for the kernel's last-known-information processing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObjectChange {
    pub next: GameObjects,
    pub before: Option<GameObject>,
    pub after: GameObject,
}
/// An actual same-incarnation controller transition at one rules cut.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ControllerChange {
    /// Physical snapshot before the effective controller changed.
    pub before: GameObject,
    /// Physical snapshot after the change, including the new control epoch.
    pub after: GameObject,
}
#[derive(Clone, Copy)]
enum ControllerUpdate {
    Baseline,
    Effective,
}

/// Actual physical movement evidence without ownership of another registry.
/// These native snapshots require projection before exposure to a player.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObjectMovement {
    pub before: GameObject,
    pub after: GameObject,
}

/// A legal attempted move may do nothing for a token that already left the
/// battlefield (CR 111.8). That is distinct from a rejected transition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ZoneMoveOutcome {
    Moved {
        movement: ObjectMovement,
    },
    Unchanged {
        object: GameObject,
        reason: UnchangedMoveReason,
    },
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// Rules reason for a successful attempted move that did not change zones.
pub enum UnchangedMoveReason {
    DepartedToken,
}
impl ZoneMoveOutcome {
    /// Require an actual movement after a caller established that premise.
    ///
    /// # Errors
    /// Returns `UnexpectedUnchangedMove` for an unchanged rules outcome.
    pub fn into_moved(self) -> Result<ObjectMovement, ObjectIdentityError> {
        match self {
            Self::Moved { movement } => Ok(movement),
            Self::Unchanged { .. } => Err(ObjectIdentityError::UnexpectedUnchangedMove),
        }
    }
}
/// Successor plus the actual result, including legal unchanged work.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ZoneMoveResult {
    pub next: GameObjects,
    pub outcome: ZoneMoveOutcome,
}
impl ZoneMoveResult {
    /// Adapter for validated card/BF operations that require actual movement.
    ///
    /// # Errors
    /// Returns `UnexpectedUnchangedMove` rather than fabricating before/after
    /// evidence when the attempted operation did not move its object.
    pub fn require_moved(self) -> Result<ObjectChange, ObjectIdentityError> {
        let movement = self.outcome.into_moved()?;
        Ok(ObjectChange {
            next: self.next,
            before: Some(movement.before),
            after: movement.after,
        })
    }
}
/// One registry successor and its actual newly inserted objects in input order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObjectInsertions {
    pub next: GameObjects,
    pub objects: Vec<GameObject>,
}

/// Actual cessation, separate from creation or movement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObjectRemoval {
    pub next: GameObjects,
    pub object: GameObject,
}

/// Physical objects leaving with their owners, in ascending incarnation order.
/// These are last-known snapshots, not zone-movement or token-cessation events.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnedObjectDepartures {
    /// Registry after removal of exactly the selected owners' current objects.
    pub next: GameObjects,
    /// Original snapshots for the enclosing kernel's departure publication.
    pub objects: Vec<GameObject>,
}

/// Current-operation object status evidence without another registry owner.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObjectStatusChange {
    pub before: GameObject,
    pub after: GameObject,
}

/// Rejections and operational exhaustion do not choose a game result.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ObjectIdentityError {
    Entropy(EntropyError),
    InvalidLibraryOrder,
    InconsistentLibrary,
    InconsistentOrderedZone,
    InconsistentRemoval,
    NegativeDrawCount,
    StaleRevision,
    RevisionExhausted,
    IdentityExhausted,
    CardAlreadyPresent,
    NotCard,
    UnknownObject,
    SameZone,
    NoController,
    NotPermanent,
    StatusUnchanged,
    InvalidTokenDefinition(DefinitionError),
    InconsistentTokenBatch,
    TokenCapacity,
    NotDepartedToken,
    UnexpectedUnchangedMove,
}

impl GameObjects {
    pub fn new(revision: u64) -> Self {
        Self {
            revision,
            next_identity: 1,
            current: BTreeMap::new(),
            by_card: BTreeMap::new(),
            ordered_zones: BTreeMap::new(),
        }
    }
    pub fn revision(&self) -> u64 {
        self.revision
    }
    /// Next checked incarnation allocation, for privileged native inspection.
    pub fn next_identity_sequence(&self) -> u64 {
        self.next_identity
    }
    pub fn get(&self, id: ObjectId) -> Option<&GameObject> {
        self.current.get(&id)
    }
    /// Canonical native objects; callers must project these before any client exposure.
    pub fn iter(&self) -> impl Iterator<Item = &GameObject> {
        self.current.values()
    }
    /// Privileged top-to-bottom current incarnations, never a public seat view.
    pub fn library(&self, owner: SeatId) -> &[ObjectId] {
        self.ordered_zone(OrderedZone::Library(owner))
    }
    /// Privileged top-to-bottom physical cards, never a public seat view or a
    /// complete pile of transient token and noncard objects.
    pub fn graveyard(&self, owner: SeatId) -> &[ObjectId] {
        self.ordered_zone(OrderedZone::Graveyard(owner))
    }
    fn ordered_zone(&self, zone: OrderedZone) -> &[ObjectId] {
        self.ordered_zones
            .get(&zone)
            .map(Vec::as_slice)
            .unwrap_or(&[])
    }
    /// Trusted authorized shuffle, with no ambient RNG or public order exposure.
    pub fn shuffle_library(
        &self,
        expected: u64,
        owner: SeatId,
        entropy: &EntropyTape,
    ) -> Result<LibraryShuffle, ObjectIdentityError> {
        self.next_revision(expected)?;
        let before = self.library(owner).to_vec();
        let (after, next_entropy) = entropy
            .shuffled(before.clone())
            .map_err(ObjectIdentityError::Entropy)?;
        let next = self.reorder_library(expected, owner, after.clone())?;
        Ok(LibraryShuffle {
            next,
            record: ShuffleRecord {
                player: owner,
                algorithm: SHUFFLE_ALGORITHM,
                before,
                after,
                entropy_start: entropy.position(),
                entropy_end: next_entropy.position(),
            },
            entropy: next_entropy,
        })
    }
    fn insert_ordered_zone(&mut self, id: ObjectId, owner: SeatId, destination: ObjectDestination) {
        match destination {
            ObjectDestination::LibraryTop => self
                .ordered_zones
                .entry(OrderedZone::Library(owner))
                .or_default()
                .insert(0, id),
            ObjectDestination::LibraryBottom => self
                .ordered_zones
                .entry(OrderedZone::Library(owner))
                .or_default()
                .push(id),
            ObjectDestination::Graveyard => self
                .ordered_zones
                .entry(OrderedZone::Graveyard(owner))
                .or_default()
                .insert(0, id),
            _ => {}
        }
    }
    /// Trusted exact permutation after rules/choice authorization. Reordering an
    /// existing library is not a zone change and preserves every incarnation.
    pub fn reorder_library(
        &self,
        expected: u64,
        owner: SeatId,
        order: Vec<ObjectId>,
    ) -> Result<Self, ObjectIdentityError> {
        let revision = self.next_revision(expected)?;
        let current = self.library(owner);
        if current.len() != order.len()
            || order.iter().copied().collect::<BTreeSet<_>>() != current.iter().copied().collect()
        {
            return Err(ObjectIdentityError::InvalidLibraryOrder);
        }
        let mut next = self.clone();
        next.ordered_zones
            .insert(OrderedZone::Library(owner), order);
        next.revision = revision;
        Ok(next)
    }
    /// Ordinary unreplaced draws, one card at a time. All successful movement
    /// uses the same zone/identity path; no SBA occurs between individual draws.
    pub fn draw_cards(
        &self,
        expected: u64,
        player: SeatId,
        amount: RulesInteger,
    ) -> Result<LibraryDraw, ObjectIdentityError> {
        if expected != self.revision {
            return Err(ObjectIdentityError::StaleRevision);
        }
        if amount < 0.into() {
            return Err(ObjectIdentityError::NegativeDrawCount);
        }
        let mut next = self.clone();
        let mut journal = DrawJournal::default();
        let mut remaining = amount;
        while remaining > 0.into() {
            let Some(movement) = next.draw_card_in_place(player)? else {
                journal.events.push(DrawEvent::EmptyLibrary {
                    player,
                    attempts: remaining,
                });
                break;
            };
            journal.record_draw(player, &movement);
            remaining = remaining.minus(&1.into());
        }
        Ok(LibraryDraw { next, journal })
    }
    /// One actual unreplaced physical draw on a transaction-local registry.
    /// Empty libraries consume neither allocator; callers group failed attempts.
    /// A caller must discard its whole tentative registry if this returns an error.
    pub(crate) fn draw_card_in_place(
        &mut self,
        player: SeatId,
    ) -> Result<Option<ObjectMovement>, ObjectIdentityError> {
        let Some(top) = self.library(player).first().copied() else {
            return Ok(None);
        };
        self.apply_zone_move_in_place(self.revision(), top, ObjectDestination::Hand)
            .and_then(ZoneMoveOutcome::into_moved)
            .map(Some)
    }
    /// Administrative native lookup only. Rules targets must retain ObjectId,
    /// never follow this stable bookkeeping key through a zone change.
    pub fn current_identity(&self, card: CardId) -> Option<ObjectId> {
        self.by_card.get(&card).copied()
    }
    fn next_revision(&self, expected: u64) -> Result<u64, ObjectIdentityError> {
        if expected != self.revision {
            return Err(ObjectIdentityError::StaleRevision);
        }
        self.revision
            .checked_add(1)
            .ok_or(ObjectIdentityError::RevisionExhausted)
    }
    fn allocate_identity(&self) -> Result<(ObjectId, u64), ObjectIdentityError> {
        let next = self
            .next_identity
            .checked_add(1)
            .ok_or(ObjectIdentityError::IdentityExhausted)?;
        Ok((ObjectId(self.next_identity), next))
    }
    /// Trusted setup after the enclosing kernel establishes an ordinary card's owner
    /// and placement. No deck legality, participant validation or ownership grant occurs.
    /// Seeds are insertion chronology: each later graveyard seed is above earlier ones.
    pub fn seed_card(
        &self,
        expected: u64,
        card: CardId,
        owner: SeatId,
        destination: ObjectDestination,
    ) -> Result<ObjectChange, ObjectIdentityError> {
        let revision = self.next_revision(expected)?;
        if self.by_card.contains_key(&card) {
            return Err(ObjectIdentityError::CardAlreadyPresent);
        }
        let (id, next_identity) = self.allocate_identity()?;
        let (zone, controller) = destination.placement(owner);
        let after = GameObject {
            id,
            origin: ObjectOrigin::Card(card),
            owner,
            zone,
            controller,
            default_controller: controller,
            tapped: (zone == ObjectZone::Battlefield).then_some(false),
            control_history: (zone == ObjectZone::Battlefield)
                .then(|| ControlHistory::new(revision)),
        };
        let mut next = self.clone();
        next.revision = revision;
        next.next_identity = next_identity;
        next.current.insert(id, after.clone());
        next.by_card.insert(card, id);
        next.insert_ordered_zone(id, owner, destination);
        Ok(ObjectChange {
            next,
            before: None,
            after,
        })
    }
    /// Trusted singular convenience over the same atomic token insertion core.
    ///
    /// # Errors
    /// Rejects stale/exhausted allocators, reservation failure or an invalid
    /// materialized permanent definition, preserving the original registry.
    pub fn insert_token(
        &self,
        expected: u64,
        creator: SeatId,
        definition: Program,
        vocabulary: &spellwake_contracts::creature_types::CreatureTypeVocabulary,
    ) -> Result<ObjectChange, ObjectIdentityError> {
        let created = self.insert_tokens(expected, vec![(creator, definition)], vocabulary)?;
        let [after]: [GameObject; 1] = created
            .objects
            .try_into()
            .map_err(|_| ObjectIdentityError::InconsistentTokenBatch)?;
        Ok(ObjectChange {
            next: created.next,
            before: None,
            after,
        })
    }

    /// Insert a complete trusted batch and return actual objects in supplied order.
    /// Each birth consumes one registry revision/incarnation, with no card index.
    /// An empty batch consumes neither allocator; stale input still rejects.
    ///
    /// # Errors
    /// Validates all definitions and allocator ranges before insertion. A rejected
    /// member, overflow or failed output reservation leaves this registry unchanged.
    /// Participant/effect legality belongs to the enclosing executor.
    pub fn insert_tokens(
        &self,
        expected: u64,
        tokens: Vec<(SeatId, Program)>,
        vocabulary: &spellwake_contracts::creature_types::CreatureTypeVocabulary,
    ) -> Result<ObjectInsertions, ObjectIdentityError> {
        if expected != self.revision {
            return Err(ObjectIdentityError::StaleRevision);
        }
        let count =
            u64::try_from(tokens.len()).map_err(|_| ObjectIdentityError::IdentityExhausted)?;
        let final_revision = self
            .revision
            .checked_add(count)
            .ok_or(ObjectIdentityError::RevisionExhausted)?;
        let final_identity = self
            .next_identity
            .checked_add(count)
            .ok_or(ObjectIdentityError::IdentityExhausted)?;
        for (_, definition) in &tokens {
            definition
                .validate(vocabulary)
                .map_err(ObjectIdentityError::InvalidTokenDefinition)?;
            if definition.permanent().is_none() || definition.names.len() != 1 {
                return Err(ObjectIdentityError::InvalidTokenDefinition(
                    DefinitionError::InvalidInstruction,
                ));
            }
        }
        let mut objects = Vec::new();
        objects
            .try_reserve_exact(tokens.len())
            .map_err(|_| ObjectIdentityError::TokenCapacity)?;
        let mut next = self.clone();
        for ((creator, definition), identity) in
            tokens.into_iter().zip(self.next_identity..final_identity)
        {
            next.revision = next
                .revision
                .checked_add(1)
                .ok_or(ObjectIdentityError::RevisionExhausted)?;
            let object = GameObject {
                id: ObjectId(identity),
                origin: ObjectOrigin::Token {
                    definition: Box::new(definition),
                },
                owner: creator,
                zone: ObjectZone::Battlefield,
                controller: Some(creator),
                default_controller: Some(creator),
                tapped: Some(false),
                control_history: Some(ControlHistory::new(next.revision)),
            };
            next.current.insert(object.id, object.clone());
            objects.push(object);
        }
        next.revision = final_revision;
        next.next_identity = final_identity;
        Ok(ObjectInsertions { next, objects })
    }
    /// Apply an otherwise legal, replacement-adjusted move. Every zone change gets
    /// a fresh incarnation. Reexiling and putting a card into the command zone again
    /// also refresh identity (CR 400.8/400.10). Same-zone library/hand/graveyard
    /// rearrangements are outside this operation; they must not renew identity.
    /// CR 400.9 command-zone face changes require later face-state processing.
    pub fn apply_zone_move(
        &self,
        expected: u64,
        object: ObjectId,
        destination: ObjectDestination,
    ) -> Result<ZoneMoveResult, ObjectIdentityError> {
        let mut next = self.clone();
        let outcome = next.apply_zone_move_in_place(expected, object, destination)?;
        Ok(ZoneMoveResult { next, outcome })
    }
    /// Trusted application-order movements, atomic across the entire batch.
    /// Each actual movement consumes one revision and incarnation. Unchanged
    /// token attempts and empty batches consume neither allocator.
    pub(crate) fn apply_zone_moves(
        &self,
        expected: u64,
        moves: &[(ObjectId, ObjectDestination)],
    ) -> Result<(Self, Vec<ZoneMoveOutcome>), ObjectIdentityError> {
        if expected != self.revision {
            return Err(ObjectIdentityError::StaleRevision);
        }
        let mut next = self.clone();
        let mut movements = Vec::with_capacity(moves.len());
        for &(object, destination) in moves {
            movements.push(next.apply_zone_move_in_place(next.revision, object, destination)?);
        }
        Ok((next, movements))
    }
    fn apply_zone_move_in_place(
        &mut self,
        expected: u64,
        object: ObjectId,
        destination: ObjectDestination,
    ) -> Result<ZoneMoveOutcome, ObjectIdentityError> {
        if expected != self.revision {
            return Err(ObjectIdentityError::StaleRevision);
        }
        if let Some(object) = self.get(object)
            && object.is_token()
            && object.zone() != ObjectZone::Battlefield
        {
            return Ok(ZoneMoveOutcome::Unchanged {
                object: object.clone(),
                reason: UnchangedMoveReason::DepartedToken,
            });
        }
        let revision = self.next_revision(expected)?;
        let before = self
            .get(object)
            .ok_or(ObjectIdentityError::UnknownObject)?
            .clone();
        let (zone, controller) = destination.placement(before.owner);
        if zone == before.zone && !matches!(zone, ObjectZone::Exile | ObjectZone::Command) {
            return Err(ObjectIdentityError::SameZone);
        }
        let (id, next_identity) = self.allocate_identity()?;
        let after = GameObject {
            id,
            origin: before.origin.clone(),
            owner: before.owner,
            zone,
            controller,
            default_controller: if before.zone == ObjectZone::Stack
                && zone == ObjectZone::Battlefield
            {
                before.default_controller
            } else {
                controller
            },
            tapped: (zone == ObjectZone::Battlefield).then_some(false),
            control_history: (zone == ObjectZone::Battlefield)
                .then(|| ControlHistory::new(revision)),
        };
        if let Some(zone) =
            OrderedZone::from_zone(before.zone).filter(|_| before.card_id().is_some())
        {
            let pile = self
                .ordered_zones
                .get_mut(&zone)
                .ok_or_else(|| zone.inconsistency())?;
            let index = pile
                .iter()
                .position(|id| *id == object)
                .ok_or_else(|| zone.inconsistency())?;
            pile.remove(index);
        }
        self.revision = revision;
        self.next_identity = next_identity;
        self.current.remove(&object);
        self.current.insert(id, after.clone());
        if let Some(card) = before.card_id() {
            self.by_card.insert(card, id);
            self.insert_ordered_zone(id, before.owner, destination);
        }
        Ok(ZoneMoveOutcome::Moved {
            movement: ObjectMovement { before, after },
        })
    }
    /// Remove an exact token already outside the battlefield (CR 704.5d).
    /// The caller owns state-action timing and publication; no zone move occurs.
    ///
    /// # Errors
    /// Rejects stale/exhausted revisions, missing objects, physical cards and
    /// battlefield tokens. No rejection changes this registry or its allocator.
    pub fn cease_token(
        &self,
        expected: u64,
        object: ObjectId,
    ) -> Result<ObjectRemoval, ObjectIdentityError> {
        let revision = self.next_revision(expected)?;
        let object = self.get(object).ok_or(ObjectIdentityError::UnknownObject)?;
        if !object.is_token() || object.zone() == ObjectZone::Battlefield {
            return Err(ObjectIdentityError::NotDepartedToken);
        }
        let mut next = self.clone();
        next.remove_current_objects(std::slice::from_ref(object))?;
        next.revision = revision;
        Ok(ObjectRemoval {
            next,
            object: object.clone(),
        })
    }
    /// Remove current objects owned by the selected departing players (CR800.4a).
    ///
    /// The enclosing kernel must authenticate departure, roster and timing, then
    /// end control effects and reconcile stack/work/resources. This operation
    /// performs only physical registry removal in the maintained zones; it does
    /// not exile survivor-owned objects controlled by a departing player.
    ///
    /// Each removed object consumes one registry revision, and no incarnation
    /// identities. Empty selections or owners with no objects are unchanged,
    /// including at maximum revision. Expected revision is checked even then.
    /// Original snapshots are ordered by ObjectId, independently of owner input.
    /// No ordinary zone movement or new incarnation is fabricated.
    ///
    /// # Errors
    /// Rejects stale revision, insufficient revision capacity or inconsistent
    /// current/card/ordered-pile indexes. All rejection leaves this registry intact.
    pub fn remove_owned_objects(
        &self,
        expected: u64,
        owners: &BTreeSet<SeatId>,
    ) -> Result<OwnedObjectDepartures, ObjectIdentityError> {
        if expected != self.revision {
            return Err(ObjectIdentityError::StaleRevision);
        }
        let objects: Vec<_> = self
            .current
            .values()
            .filter(|object| owners.contains(&object.owner()))
            .cloned()
            .collect();
        let count =
            u64::try_from(objects.len()).map_err(|_| ObjectIdentityError::RevisionExhausted)?;
        let revision = self
            .revision
            .checked_add(count)
            .ok_or(ObjectIdentityError::RevisionExhausted)?;
        let mut next = self.clone();
        next.remove_current_objects(&objects)?;
        next.revision = revision;
        Ok(OwnedObjectDepartures { next, objects })
    }

    /// One shared physical-removal owner. Validate all indexes before mutation.
    /// It neither allocates nor publishes; callers own revision accounting.
    fn remove_current_objects(
        &mut self,
        objects: &[GameObject],
    ) -> Result<(), ObjectIdentityError> {
        let mut ordered = BTreeSet::new();
        for (zone, pile) in &self.ordered_zones {
            for id in pile {
                let Some(object) = self.current.get(id) else {
                    return Err(ObjectIdentityError::InconsistentRemoval);
                };
                if object.card_id().is_none()
                    || OrderedZone::from_zone(object.zone()) != Some(*zone)
                    || !ordered.insert(*id)
                {
                    return Err(ObjectIdentityError::InconsistentRemoval);
                }
            }
        }
        for (id, object) in &self.current {
            if matches!(object.zone(), ObjectZone::Library(owner) | ObjectZone::Hand(owner) | ObjectZone::Graveyard(owner) if owner != object.owner())
                || *id != object.id()
                || object
                    .card_id()
                    .is_some_and(|card| self.by_card.get(&card) != Some(id))
                || ordered.contains(id)
                    != (object.card_id().is_some()
                        && OrderedZone::from_zone(object.zone()).is_some())
            {
                return Err(ObjectIdentityError::InconsistentRemoval);
            }
        }
        if self
            .by_card
            .iter()
            .any(|(card, id)| self.current.get(id).and_then(GameObject::card_id) != Some(*card))
        {
            return Err(ObjectIdentityError::InconsistentRemoval);
        }
        let removed: BTreeSet<_> = objects.iter().map(GameObject::id).collect();
        if removed.len() != objects.len()
            || objects
                .iter()
                .any(|object| self.current.get(&object.id()) != Some(object))
        {
            return Err(ObjectIdentityError::InconsistentRemoval);
        }
        self.current.retain(|id, _| !removed.contains(id));
        self.by_card.retain(|_, id| !removed.contains(id));
        for pile in self.ordered_zones.values_mut() {
            pile.retain(|id| !removed.contains(id));
        }
        Ok(())
    }
    /// Trusted completed status change, preserving incarnation. The enclosing
    /// kernel establishes effect/cost legality; this is not a player action.
    pub fn set_tapped(
        &self,
        expected: u64,
        object: ObjectId,
        tapped: bool,
    ) -> Result<ObjectChange, ObjectIdentityError> {
        let mut next = self.clone();
        let change = next.set_tapped_in_place(expected, object, tapped)?;
        Ok(ObjectChange {
            next,
            before: Some(change.before),
            after: change.after,
        })
    }
    /// Atomic status changes in supplied order. Each item consumes one revision;
    /// an empty batch validates its basis without consuming a registry revision.
    pub(crate) fn set_tapped_batch(
        &self,
        expected: u64,
        objects: &[ObjectId],
        tapped: bool,
    ) -> Result<(Self, Vec<ObjectStatusChange>), ObjectIdentityError> {
        if expected != self.revision {
            return Err(ObjectIdentityError::StaleRevision);
        }
        let mut next = self.clone();
        let mut changes = Vec::with_capacity(objects.len());
        for &object in objects {
            changes.push(next.set_tapped_in_place(next.revision, object, tapped)?);
        }
        Ok((next, changes))
    }
    fn set_tapped_in_place(
        &mut self,
        expected: u64,
        object: ObjectId,
        tapped: bool,
    ) -> Result<ObjectStatusChange, ObjectIdentityError> {
        let revision = self.next_revision(expected)?;
        let card = self
            .current
            .get_mut(&object)
            .ok_or(ObjectIdentityError::UnknownObject)?;
        let prior = card.tapped.ok_or(ObjectIdentityError::NotPermanent)?;
        if prior == tapped {
            return Err(ObjectIdentityError::StatusUnchanged);
        }
        let before = card.clone();
        card.tapped = Some(tapped);
        let after = card.clone();
        self.revision = revision;
        Ok(ObjectStatusChange { before, after })
    }
    /// Trusted actual controller-turn boundary, independent from untapping.
    /// Stamp all currently controlled battlefield permanents without changing
    /// identities, epochs, status or zone order. The enclosing window validates
    /// participant and lifecycle authority; this primitive has no seat registry.
    pub fn record_turn_start(
        &self,
        expected: u64,
        controller: SeatId,
    ) -> Result<Self, ObjectIdentityError> {
        let revision = self.next_revision(expected)?;
        let mut next = self.clone();
        for card in next.current.values_mut() {
            if card.zone == ObjectZone::Battlefield
                && card.controller == Some(controller)
                && let Some(history) = card.control_history.as_mut()
            {
                history.turn_start_epoch = Some(history.epoch);
            }
        }
        next.revision = revision;
        Ok(next)
    }
    #[cfg(test)]
    pub(crate) fn set_test_revision(&mut self, revision: u64) {
        self.revision = revision;
    }
    #[cfg(test)]
    pub(crate) fn set_test_next_identity(&mut self, next_identity: u64) {
        self.next_identity = next_identity;
    }
    /// Establish a trusted controller baseline and current controller, preserving
    /// incarnation. Ordinary continuous effects use the internal effective-control
    /// continuation instead. This is not an authorized player action.
    ///
    /// # Errors
    /// Rejects stale/exhausted revisions, unknown incarnations and zones without
    /// a controller. Participant validation belongs to the enclosing kernel.
    pub fn apply_controller(
        &self,
        expected: u64,
        object: ObjectId,
        controller: SeatId,
    ) -> Result<ObjectChange, ObjectIdentityError> {
        self.update_controller(expected, object, controller, ControllerUpdate::Baseline)
    }
    pub(crate) fn apply_effective_controller(
        &self,
        expected: u64,
        object: ObjectId,
        controller: SeatId,
    ) -> Result<ObjectChange, ObjectIdentityError> {
        self.update_controller(expected, object, controller, ControllerUpdate::Effective)
    }
    /// One atomic battlefield batch under this sole physical registry owner.
    pub(crate) fn apply_effective_controllers(
        &self,
        expected: u64,
        controllers: &BTreeMap<ObjectId, SeatId>,
    ) -> Result<(Self, Vec<ControllerChange>), ObjectIdentityError> {
        if expected != self.revision {
            return Err(ObjectIdentityError::StaleRevision);
        }
        for &object in controllers.keys() {
            if self
                .get(object)
                .ok_or(ObjectIdentityError::UnknownObject)?
                .zone()
                != ObjectZone::Battlefield
            {
                return Err(ObjectIdentityError::NotPermanent);
            }
        }
        let mut next = self.clone();
        let mut changes = Vec::with_capacity(controllers.len());
        for (&object, &controller) in controllers {
            if next
                .get(object)
                .ok_or(ObjectIdentityError::UnknownObject)?
                .controller()
                == Some(controller)
            {
                continue;
            }
            let change = next.apply_effective_controller(next.revision(), object, controller)?;
            changes.push(ControllerChange {
                before: change.before.ok_or(ObjectIdentityError::UnknownObject)?,
                after: change.after,
            });
            next = change.next;
        }
        Ok((next, changes))
    }
    fn update_controller(
        &self,
        expected: u64,
        object: ObjectId,
        controller: SeatId,
        update: ControllerUpdate,
    ) -> Result<ObjectChange, ObjectIdentityError> {
        let revision = self.next_revision(expected)?;
        let before = self.get(object).ok_or(ObjectIdentityError::UnknownObject)?;
        if !matches!(before.zone, ObjectZone::Battlefield | ObjectZone::Stack) {
            return Err(ObjectIdentityError::NoController);
        }
        let after = GameObject {
            controller: Some(controller),
            default_controller: match update {
                ControllerUpdate::Baseline => Some(controller),
                ControllerUpdate::Effective => before.default_controller,
            },
            control_history: if before.zone == ObjectZone::Battlefield
                && before.controller != Some(controller)
            {
                Some(ControlHistory::new(revision))
            } else {
                before.control_history
            },
            ..before.clone()
        };
        let mut next = self.clone();
        next.revision = revision;
        next.current.insert(object, after.clone());
        Ok(ObjectChange {
            next,
            before: Some(before.clone()),
            after,
        })
    }
}

#[cfg(test)]
mod batch_tests;

#[cfg(test)]
mod tests {
    use super::*;
    fn pile_fixture() -> GameObjects {
        let mut cards = status_fixture();
        for (card, owner) in [(91, OWNER), (7, OTHER), (43, OWNER)] {
            cards = cards
                .seed_card(
                    cards.revision(),
                    CardId(card),
                    owner,
                    ObjectDestination::Graveyard,
                )
                .unwrap()
                .next;
        }
        cards
    }
    fn pile_cards(cards: &GameObjects, pile: &[ObjectId]) -> Vec<CardId> {
        pile.iter()
            .map(|id| {
                cards
                    .get(*id)
                    .unwrap()
                    .card_id()
                    .expect("physical fixture card")
            })
            .collect()
    }
    #[test]
    fn graveyard_seeds_are_explicit_arrival_chronology_per_owner() {
        let cards = pile_fixture();
        assert_eq!(
            pile_cards(&cards, cards.graveyard(OWNER)),
            vec![CardId(43), CardId(91)]
        );
        assert_eq!(pile_cards(&cards, cards.graveyard(OTHER)), vec![CardId(7)]);
        assert_eq!(
            pile_cards(&cards, cards.library(OWNER)),
            vec![CardId(21), CardId(22)]
        );
        assert!(cards.graveyard(SeatId(99)).is_empty());
        assert_eq!(
            cards.graveyard(OWNER)[0],
            cards.current_identity(CardId(43)).unwrap()
        );
    }
    #[test]
    fn graveyard_departure_removes_exact_incarnation_and_reentry_arrives_on_top() {
        let cards = pile_fixture();
        let old = cards.current_identity(CardId(91)).unwrap();
        let saved_top = cards.graveyard(OWNER)[0];
        let departed = cards
            .apply_zone_move(cards.revision(), old, ObjectDestination::Hand)
            .unwrap()
            .require_moved()
            .unwrap();
        assert_eq!(departed.next.graveyard(OWNER), &[saved_top]);
        assert_eq!(departed.next.graveyard(OTHER), cards.graveyard(OTHER));
        assert_eq!(departed.next.library(OWNER), cards.library(OWNER));
        let entered = departed
            .next
            .apply_zone_move(
                departed.next.revision(),
                departed.after.id(),
                ObjectDestination::Graveyard,
            )
            .unwrap()
            .require_moved()
            .unwrap();
        assert_eq!(
            entered.next.graveyard(OWNER),
            &[entered.after.id(), saved_top]
        );
        assert_ne!(entered.after.id(), old);
        assert_ne!(entered.after.id(), departed.after.id());
        assert!(entered.next.get(old).is_none());
        assert!(entered.next.get(departed.after.id()).is_none());
        assert_eq!(
            entered.next.current_identity(CardId(91)),
            Some(entered.after.id())
        );
        let before = entered.next.clone();
        assert_eq!(
            entered
                .next
                .apply_zone_move(entered.next.revision(), old, ObjectDestination::Exile),
            Err(ObjectIdentityError::UnknownObject)
        );
        assert_eq!(
            entered.next.apply_zone_move(
                entered.next.revision(),
                entered.after.id(),
                ObjectDestination::Graveyard
            ),
            Err(ObjectIdentityError::SameZone)
        );
        assert_eq!(entered.next, before);
    }
    #[test]
    fn zone_batch_returns_application_evidence_and_preserves_old_pile_segments() {
        let cards = pile_fixture();
        let objects = [11, 13, 12].map(|id| cards.current_identity(CardId(id)).unwrap());
        let moves = objects.map(|id| (id, ObjectDestination::Graveyard));
        let before = cards.clone();
        let (next, movements) = cards.apply_zone_moves(cards.revision(), &moves).unwrap();
        let movements = movements
            .into_iter()
            .map(ZoneMoveOutcome::into_moved)
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(cards, before);
        assert_eq!(next.revision(), cards.revision() + 3);
        assert_eq!(next.next_identity, cards.next_identity + 3);
        assert_eq!(movements.len(), 3);
        for (index, (movement, original)) in movements.iter().zip(objects).enumerate() {
            assert_eq!(&movement.before, cards.get(original).unwrap());
            assert_eq!(
                movement.after.id(),
                ObjectId(cards.next_identity + index as u64)
            );
            assert_eq!(
                movement.after.card_id().expect("physical fixture card"),
                movement.before.card_id().expect("physical fixture card")
            );
            assert_eq!(
                movement.after.zone(),
                ObjectZone::Graveyard(movement.before.owner())
            );
            assert_eq!(movement.after.controller(), None);
            assert_eq!(movement.after.tapped(), None);
            assert_eq!(movement.after.control_epoch(), None);
            assert_eq!(next.get(movement.after.id()), Some(&movement.after));
            assert!(next.get(original).is_none());
        }
        assert_eq!(
            pile_cards(&next, next.graveyard(OWNER)),
            vec![CardId(11), CardId(43), CardId(91)]
        );
        assert_eq!(
            pile_cards(&next, next.graveyard(OTHER)),
            vec![CardId(12), CardId(13), CardId(7)]
        );
        assert_eq!(&next.graveyard(OWNER)[1..], cards.graveyard(OWNER));
        assert_eq!(&next.graveyard(OTHER)[2..], cards.graveyard(OTHER));
        assert_eq!(next.library(OWNER), cards.library(OWNER));
    }
    #[test]
    fn one_batch_can_move_between_ordered_piles_and_enter_at_the_correct_revision() {
        let cards = pile_fixture();
        let objects = [21, 13, 43, 22].map(|id| cards.current_identity(CardId(id)).unwrap());
        let moves = [
            (objects[0], ObjectDestination::Graveyard),
            (objects[1], ObjectDestination::LibraryTop),
            (objects[2], ObjectDestination::LibraryBottom),
            (
                objects[3],
                ObjectDestination::Battlefield { controller: OTHER },
            ),
        ];
        let (next, movements) = cards.apply_zone_moves(cards.revision(), &moves).unwrap();
        let movements = movements
            .into_iter()
            .map(ZoneMoveOutcome::into_moved)
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(pile_cards(&next, next.library(OWNER)), vec![CardId(43)]);
        assert_eq!(pile_cards(&next, next.library(OTHER)), vec![CardId(13)]);
        assert_eq!(
            pile_cards(&next, next.graveyard(OWNER)),
            vec![CardId(21), CardId(91)]
        );
        assert_eq!(next.graveyard(OTHER), cards.graveyard(OTHER));
        assert_eq!(next.revision(), cards.revision() + 4);
        assert_eq!(
            movements[3].after.control_epoch(),
            Some(ControlEpoch(cards.revision() + 4))
        );
        assert_eq!(movements[3].after.controller(), Some(OTHER));
        assert_eq!(movements[3].after.owner(), OWNER);
        for original in objects {
            assert!(next.get(original).is_none());
        }
    }
    #[test]
    fn late_zone_batch_validation_failures_keep_every_index_and_allocator() {
        let cards = pile_fixture();
        let first = cards.current_identity(CardId(11)).unwrap();
        let hand = cards.current_identity(CardId(13)).unwrap();
        for (last, destination, error) in [
            (
                ObjectId(999),
                ObjectDestination::Exile,
                ObjectIdentityError::UnknownObject,
            ),
            (
                first,
                ObjectDestination::Exile,
                ObjectIdentityError::UnknownObject,
            ),
            (hand, ObjectDestination::Hand, ObjectIdentityError::SameZone),
        ] {
            let before = cards.clone();
            assert_eq!(
                cards.apply_zone_moves(
                    cards.revision(),
                    &[(first, ObjectDestination::Graveyard), (last, destination)]
                ),
                Err(error)
            );
            assert_eq!(cards, before);
        }
        assert_eq!(
            cards.apply_zone_moves(
                cards.revision() - 1,
                &[(first, ObjectDestination::Graveyard)]
            ),
            Err(ObjectIdentityError::StaleRevision)
        );
        for (object, zone, error) in [
            (
                cards.library(OWNER)[0],
                OrderedZone::Library(OWNER),
                ObjectIdentityError::InconsistentLibrary,
            ),
            (
                cards.graveyard(OWNER)[0],
                OrderedZone::Graveyard(OWNER),
                ObjectIdentityError::InconsistentOrderedZone,
            ),
        ] {
            let mut inconsistent = cards.clone();
            inconsistent.ordered_zones.get_mut(&zone).unwrap().clear();
            let before = inconsistent.clone();
            assert_eq!(
                inconsistent.apply_zone_moves(
                    inconsistent.revision(),
                    &[
                        (first, ObjectDestination::Graveyard),
                        (object, ObjectDestination::Hand)
                    ]
                ),
                Err(error)
            );
            assert_eq!(inconsistent, before);
        }
    }
    #[test]
    fn zone_batch_late_allocator_exhaustion_discards_prior_arrivals() {
        for identity_fault in [true, false] {
            let mut cards = pile_fixture();
            let first = cards.current_identity(CardId(11)).unwrap();
            let second = cards.current_identity(CardId(12)).unwrap();
            if identity_fault {
                cards.next_identity = u64::MAX - 1;
            } else {
                cards.revision = u64::MAX - 1;
            }
            let before = cards.clone();
            assert_eq!(
                cards.apply_zone_moves(
                    cards.revision(),
                    &[
                        (first, ObjectDestination::Graveyard),
                        (second, ObjectDestination::Graveyard)
                    ]
                ),
                Err(if identity_fault {
                    ObjectIdentityError::IdentityExhausted
                } else {
                    ObjectIdentityError::RevisionExhausted
                })
            );
            assert_eq!(cards, before);
        }
    }
    #[test]
    fn empty_zone_batch_validates_basis_without_consuming_exhausted_allocators() {
        let mut cards = pile_fixture();
        cards.revision = u64::MAX;
        cards.next_identity = u64::MAX;
        let before = cards.clone();
        let (next, movements) = cards.apply_zone_moves(u64::MAX, &[]).unwrap();
        assert_eq!(next, before);
        assert!(movements.is_empty());
        assert_eq!(
            cards.apply_zone_moves(u64::MAX - 1, &[]),
            Err(ObjectIdentityError::StaleRevision)
        );
        assert_eq!(cards, before);
    }
    #[test]
    fn library_reorder_shuffle_draw_and_nonmovement_keep_graveyard_order() {
        let cards = pile_fixture();
        let original = cards.clone();
        let permanent = cards.current_identity(CardId(11)).unwrap();
        let cards = cards
            .set_tapped(cards.revision(), permanent, true)
            .unwrap()
            .next;
        let cards = cards
            .apply_controller(cards.revision(), permanent, OTHER)
            .unwrap()
            .next;
        let cards = cards.record_turn_start(cards.revision(), OTHER).unwrap();
        assert_eq!(cards.ordered_zones, original.ordered_zones);
        let mut reversed = cards.library(OWNER).to_vec();
        reversed.reverse();
        let reordered = cards
            .reorder_library(cards.revision(), OWNER, reversed.clone())
            .unwrap();
        assert_eq!(reordered.library(OWNER), reversed);
        assert_eq!(reordered.graveyard(OWNER), original.graveyard(OWNER));
        assert_eq!(reordered.graveyard(OTHER), original.graveyard(OTHER));
        assert_eq!(reordered.current, cards.current);
        let shuffled = reordered
            .shuffle_library(reordered.revision(), OWNER, &EntropyTape::new(vec![0]))
            .unwrap()
            .next;
        assert_eq!(shuffled.current, reordered.current);
        assert_eq!(shuffled.graveyard(OWNER), original.graveyard(OWNER));
        assert_eq!(shuffled.graveyard(OTHER), original.graveyard(OTHER));
        let top = shuffled.library(OWNER)[0];
        let remaining = shuffled.library(OWNER)[1..].to_vec();
        let drawn = shuffled
            .draw_cards(shuffled.revision(), OWNER, 1.into())
            .unwrap();
        assert_eq!(drawn.next.library(OWNER), remaining);
        assert!(drawn.next.get(top).is_none());
        assert_eq!(drawn.next.revision(), shuffled.revision() + 1);
        assert_eq!(drawn.next.graveyard(OWNER), original.graveyard(OWNER));
        assert_eq!(drawn.next.graveyard(OTHER), original.graveyard(OTHER));
    }
    fn status_fixture() -> GameObjects {
        let mut cards = GameObjects::new(0);
        for (card, owner, destination) in [
            (
                11,
                OWNER,
                ObjectDestination::Battlefield { controller: OWNER },
            ),
            (
                12,
                OTHER,
                ObjectDestination::Battlefield { controller: OWNER },
            ),
            (13, OTHER, ObjectDestination::Hand),
            (21, OWNER, ObjectDestination::LibraryBottom),
            (22, OWNER, ObjectDestination::LibraryBottom),
        ] {
            cards = cards
                .seed_card(cards.revision(), CardId(card), owner, destination)
                .unwrap()
                .next;
        }
        cards.record_turn_start(cards.revision(), OWNER).unwrap()
    }
    #[test]
    fn status_batch_preserves_identity_history_and_library_order() {
        let mut cards = status_fixture();
        cards.next_identity = u64::MAX;
        let first = cards.current_identity(CardId(11)).unwrap();
        let second = cards.current_identity(CardId(12)).unwrap();
        let before = cards.clone();
        let (tapped, changes) = cards
            .set_tapped_batch(cards.revision(), &[second, first], true)
            .unwrap();
        assert_eq!(cards, before);
        assert_eq!(tapped.revision(), before.revision() + 2);
        assert_eq!(tapped.next_identity, u64::MAX);
        assert_eq!(tapped.by_card, before.by_card);
        assert_eq!(tapped.ordered_zones, before.ordered_zones);
        assert_eq!(changes.len(), 2);
        for (change, id) in changes.iter().zip([second, first]) {
            let original = before.get(id).unwrap();
            assert_eq!(change.before, *original);
            assert_eq!(change.after.id(), id);
            assert_eq!(
                change.after.card_id().expect("physical fixture card"),
                original.card_id().expect("physical fixture card")
            );
            assert_eq!(change.after.owner(), original.owner());
            assert_eq!(change.after.controller(), original.controller());
            assert_eq!(change.after.zone(), ObjectZone::Battlefield);
            assert_eq!(change.after.tapped(), Some(true));
            assert_eq!(change.after.control_epoch(), original.control_epoch());
            assert_eq!(
                change.after.has_continuous_control_since_turn_start(),
                Some(true)
            );
            assert_eq!(tapped.get(id), Some(&change.after));
        }
        for original in before
            .iter()
            .filter(|card| ![first, second].contains(&card.id()))
        {
            assert_eq!(tapped.get(original.id()), Some(original));
        }
        let (untapped, _) = tapped
            .set_tapped_batch(tapped.revision(), &[second, first], false)
            .unwrap();
        assert_eq!(untapped.revision(), before.revision() + 4);
        assert_eq!(untapped.current, before.current);
        assert_eq!(untapped.ordered_zones, before.ordered_zones);
    }
    #[test]
    fn late_invalid_status_items_reject_the_entire_batch() {
        let cards = status_fixture();
        let first = cards.current_identity(CardId(11)).unwrap();
        let second = cards.current_identity(CardId(12)).unwrap();
        let hand = cards.current_identity(CardId(13)).unwrap();
        let already_tapped = cards
            .set_tapped(cards.revision(), second, true)
            .unwrap()
            .next;
        for (state, objects, expected) in [
            (
                &cards,
                vec![first, ObjectId(999)],
                ObjectIdentityError::UnknownObject,
            ),
            (&cards, vec![first, hand], ObjectIdentityError::NotPermanent),
            (
                &cards,
                vec![first, first],
                ObjectIdentityError::StatusUnchanged,
            ),
            (
                &already_tapped,
                vec![first, second],
                ObjectIdentityError::StatusUnchanged,
            ),
        ] {
            let before = state.clone();
            assert_eq!(
                state.set_tapped_batch(state.revision(), &objects, true),
                Err(expected)
            );
            assert_eq!(*state, before);
            assert_eq!(state.get(first).unwrap().tapped(), Some(false));
        }
        let before = cards.clone();
        assert_eq!(
            cards.set_tapped_batch(cards.revision() - 1, &[first], true),
            Err(ObjectIdentityError::StaleRevision)
        );
        assert_eq!(cards, before);
    }
    #[test]
    fn status_batch_second_revision_failure_keeps_all_accepted_cards() {
        let mut cards = status_fixture();
        let first = cards.current_identity(CardId(11)).unwrap();
        let second = cards.current_identity(CardId(12)).unwrap();
        cards.revision = u64::MAX - 1;
        let before = cards.clone();
        assert_eq!(
            cards.set_tapped_batch(cards.revision(), &[first, second], true),
            Err(ObjectIdentityError::RevisionExhausted)
        );
        assert_eq!(cards, before);
        let (one, changes) = cards
            .set_tapped_batch(cards.revision(), &[first], true)
            .unwrap();
        assert_eq!(one.revision(), u64::MAX);
        assert_eq!(changes.len(), 1);
        assert_eq!(one.get(first).unwrap().tapped(), Some(true));
        assert_eq!(one.get(second), cards.get(second));
    }
    #[test]
    fn empty_status_batch_checks_basis_without_consuming_revision_capacity() {
        let mut cards = status_fixture();
        cards.revision = u64::MAX;
        let before = cards.clone();
        let (same, changes) = cards.set_tapped_batch(u64::MAX, &[], true).unwrap();
        assert_eq!(same, before);
        assert!(changes.is_empty());
        assert_eq!(
            cards.set_tapped_batch(u64::MAX - 1, &[], true),
            Err(ObjectIdentityError::StaleRevision)
        );
        for result in [
            cards
                .set_tapped_batch(u64::MAX, &[ObjectId(999)], true)
                .map(|_| ()),
            cards.set_tapped(u64::MAX, ObjectId(999), true).map(|_| ()),
        ] {
            assert_eq!(result, Err(ObjectIdentityError::RevisionExhausted));
        }
        assert_eq!(cards, before);
    }
    #[test]
    fn turn_start_failure_preserves_every_history_and_needs_no_identity_allocation() {
        let mut cards = seeded(ObjectDestination::Battlefield { controller: OWNER }).next;
        cards = cards
            .seed_card(
                cards.revision(),
                CardId(12),
                OTHER,
                ObjectDestination::Battlefield { controller: OTHER },
            )
            .unwrap()
            .next;
        let own = cards.current_identity(CardId(11)).unwrap();
        let other = cards.current_identity(CardId(12)).unwrap();
        cards.next_identity = u64::MAX;
        let before = cards.clone();
        assert_eq!(
            cards.record_turn_start(cards.revision() - 1, OWNER),
            Err(ObjectIdentityError::StaleRevision)
        );
        assert_eq!(cards, before);
        let marked = cards.record_turn_start(cards.revision(), OWNER).unwrap();
        assert_eq!(cards, before);
        assert_eq!(marked.next_identity, u64::MAX);
        assert_eq!(
            marked.get(own).unwrap().control_epoch(),
            cards.get(own).unwrap().control_epoch()
        );
        assert_eq!(
            marked
                .get(own)
                .unwrap()
                .has_continuous_control_since_turn_start(),
            Some(true)
        );
        assert_eq!(marked.get(other), cards.get(other));
        let mut exhausted = marked;
        exhausted.revision = u64::MAX;
        let before = exhausted.clone();
        assert_eq!(
            exhausted.record_turn_start(u64::MAX, OTHER),
            Err(ObjectIdentityError::RevisionExhausted)
        );
        assert_eq!(exhausted, before);
    }

    #[test]
    fn control_epochs_use_the_checked_revision_and_failed_changes_keep_history() {
        let mut cards = seeded(ObjectDestination::Battlefield { controller: OWNER }).next;
        let object = cards.current_identity(CardId(11)).unwrap();
        assert_eq!(
            cards.get(object).unwrap().control_epoch(),
            Some(ControlEpoch(cards.revision()))
        );
        cards = cards.record_turn_start(cards.revision(), OWNER).unwrap();
        cards.next_identity = u64::MAX;
        let before = cards.clone();
        assert_eq!(
            cards.apply_zone_move(cards.revision(), object, ObjectDestination::Hand),
            Err(ObjectIdentityError::IdentityExhausted)
        );
        assert_eq!(cards, before);
        cards.revision = u64::MAX - 1;
        let changed = cards
            .apply_controller(cards.revision(), object, OTHER)
            .unwrap();
        assert_eq!(changed.after.control_epoch(), Some(ControlEpoch(u64::MAX)));
        assert_eq!(
            changed.after.has_continuous_control_since_turn_start(),
            Some(false)
        );
        let exhausted = changed.next;
        let before = exhausted.clone();
        for controller in [OWNER, OTHER] {
            assert_eq!(
                exhausted.apply_controller(u64::MAX, object, controller),
                Err(ObjectIdentityError::RevisionExhausted)
            );
            assert_eq!(exhausted, before);
        }
    }

    #[test]
    fn single_draw_cut_moves_only_the_actual_top_card_then_stops_without_allocating() {
        let mut cards = GameObjects::new(0);
        cards = cards
            .seed_card(0, CardId(7), OWNER, ObjectDestination::LibraryBottom)
            .unwrap()
            .next;
        let before = cards.clone();
        let movement = cards.draw_card_in_place(OWNER).unwrap().unwrap();
        assert_eq!(
            movement.before,
            *before.get(before.library(OWNER)[0]).unwrap()
        );
        assert_eq!(
            movement.after.card_id().expect("physical fixture card"),
            CardId(7)
        );
        assert_eq!(movement.after.zone(), ObjectZone::Hand(OWNER));
        assert_ne!(movement.before.id(), movement.after.id());
        assert!(cards.library(OWNER).is_empty());
        assert!(cards.get(movement.before.id()).is_none());
        assert_eq!(cards.get(movement.after.id()), Some(&movement.after));
        assert_eq!(cards.revision(), before.revision() + 1);
        cards.revision = u64::MAX;
        cards.next_identity = u64::MAX;
        let empty = cards.clone();
        assert_eq!(cards.draw_card_in_place(OWNER), Ok(None));
        assert_eq!(cards, empty);
    }
    #[test]
    fn later_draw_failure_rolls_back_every_prior_card_and_order_change() {
        let mut cards = GameObjects::new(0);
        for id in [7, 3] {
            cards = cards
                .seed_card(
                    cards.revision(),
                    CardId(id),
                    SeatId(0),
                    ObjectDestination::LibraryBottom,
                )
                .unwrap()
                .next;
        }
        for identity_fault in [true, false] {
            let mut exhausted = cards.clone();
            if identity_fault {
                exhausted.next_identity = u64::MAX - 1;
            } else {
                exhausted.revision = u64::MAX - 1;
            }
            let before = exhausted.clone();
            assert_eq!(
                exhausted.draw_cards(exhausted.revision(), SeatId(0), 2.into()),
                Err(if identity_fault {
                    ObjectIdentityError::IdentityExhausted
                } else {
                    ObjectIdentityError::RevisionExhausted
                })
            );
            assert_eq!(exhausted, before);
        }
    }
    #[test]
    fn status_revision_failure_preserves_physical_status_and_incarnation() {
        let mut state = GameObjects::new(0)
            .seed_card(
                0,
                CardId(99),
                SeatId(0),
                ObjectDestination::Battlefield {
                    controller: SeatId(0),
                },
            )
            .unwrap()
            .next;
        let object = state.current_identity(CardId(99)).unwrap();
        state.next_identity = u64::MAX;
        let tapped = state.set_tapped(state.revision(), object, true).unwrap();
        assert_eq!(tapped.after.id(), object);
        assert_eq!(tapped.after.tapped(), Some(true));
        assert_eq!(tapped.next.next_identity, u64::MAX);
        assert_eq!(state.get(object).unwrap().tapped(), Some(false));
        state.revision = u64::MAX;
        let before = state.clone();
        assert_eq!(
            state.set_tapped(u64::MAX, object, true),
            Err(ObjectIdentityError::RevisionExhausted)
        );
        assert_eq!(state, before);
        assert_eq!(state.get(object).unwrap().tapped(), Some(false));
    }
    const OWNER: SeatId = SeatId(0);
    const OTHER: SeatId = SeatId(1);
    fn seeded(destination: ObjectDestination) -> ObjectChange {
        GameObjects::new(0)
            .seed_card(0, CardId(11), OWNER, destination)
            .unwrap()
    }
    #[test]
    fn a_stolen_card_returns_as_a_new_object_and_does_not_keep_its_controller() {
        let first = seeded(ObjectDestination::Battlefield { controller: OWNER });
        let original = first.after.id();
        let stolen = first.next.apply_controller(1, original, OTHER).unwrap();
        assert_eq!(stolen.after.id(), original);
        assert_eq!(stolen.after.owner(), OWNER);
        assert_eq!(stolen.after.controller(), Some(OTHER));
        let hand = stolen
            .next
            .apply_zone_move(2, original, ObjectDestination::Hand)
            .unwrap()
            .require_moved()
            .unwrap();
        assert_eq!(hand.before.as_ref().unwrap().controller(), Some(OTHER));
        assert_eq!(hand.after.zone(), ObjectZone::Hand(OWNER));
        assert_eq!(hand.after.controller(), None);
        assert!(hand.next.get(original).is_none());
        let returned = hand
            .next
            .apply_zone_move(
                3,
                hand.after.id(),
                ObjectDestination::Battlefield { controller: OWNER },
            )
            .unwrap()
            .require_moved()
            .unwrap();
        assert_ne!(original, returned.after.id());
        assert_ne!(hand.after.id(), returned.after.id());
        assert_eq!(
            returned.after.card_id().expect("physical fixture card"),
            CardId(11)
        );
        assert_eq!(returned.after.controller(), Some(OWNER));
        assert_eq!(
            returned.next.apply_controller(4, original, OTHER),
            Err(ObjectIdentityError::UnknownObject)
        );
        assert_eq!(
            returned.next.current_identity(CardId(11)),
            Some(returned.after.id())
        );
    }
    #[test]
    fn all_supported_zone_transitions_preserve_owner_and_refresh_identity() {
        // Expected placements are rules-backed literals, independent of the placement helper.
        let places = [
            (
                ObjectDestination::LibraryBottom,
                ObjectZone::Library(OWNER),
                None,
            ),
            (ObjectDestination::Hand, ObjectZone::Hand(OWNER), None),
            (
                ObjectDestination::Graveyard,
                ObjectZone::Graveyard(OWNER),
                None,
            ),
            (
                ObjectDestination::Battlefield { controller: OTHER },
                ObjectZone::Battlefield,
                Some(OTHER),
            ),
            (
                ObjectDestination::Stack { controller: OTHER },
                ObjectZone::Stack,
                Some(OTHER),
            ),
            (ObjectDestination::Exile, ObjectZone::Exile, None),
            (ObjectDestination::Command, ObjectZone::Command, None),
        ];
        for (source, source_zone, source_controller) in places {
            for (destination, zone, controller) in places {
                let start = seeded(source);
                assert_eq!(start.after.zone(), source_zone);
                assert_eq!(start.after.controller(), source_controller);
                let saved = start.next.clone();
                let result = start.next.apply_zone_move(1, start.after.id(), destination);
                if zone == start.after.zone()
                    && !matches!(zone, ObjectZone::Exile | ObjectZone::Command)
                {
                    assert_eq!(result, Err(ObjectIdentityError::SameZone));
                } else {
                    let change = result.unwrap().require_moved().unwrap();
                    assert_eq!(change.after.owner(), OWNER);
                    assert_eq!(change.after.zone(), zone);
                    assert_eq!(change.after.controller(), controller);
                    assert_ne!(change.after.id(), start.after.id());
                    assert!(change.next.get(start.after.id()).is_none());
                    assert_eq!(change.before, Some(start.after.clone()));
                }
                assert_eq!(start.next, saved);
            }
        }
    }
    #[test]
    fn two_cards_with_the_same_owner_have_distinct_current_incarnations() {
        let first = seeded(ObjectDestination::Hand);
        let second = first
            .next
            .seed_card(1, CardId(12), OWNER, ObjectDestination::Hand)
            .unwrap();
        assert_ne!(first.after.id(), second.after.id());
        assert_eq!(second.next.get(first.after.id()), Some(&first.after));
        assert_eq!(
            second
                .next
                .seed_card(2, CardId(11), OWNER, ObjectDestination::Exile),
            Err(ObjectIdentityError::CardAlreadyPresent)
        );
    }
    #[test]
    fn stale_invalid_and_exhausted_transitions_preserve_input() {
        let first = seeded(ObjectDestination::Hand);
        let saved = first.next.clone();
        assert_eq!(
            first
                .next
                .apply_zone_move(0, first.after.id(), ObjectDestination::Exile),
            Err(ObjectIdentityError::StaleRevision)
        );
        assert_eq!(
            first
                .next
                .apply_zone_move(1, ObjectId(999), ObjectDestination::Exile),
            Err(ObjectIdentityError::UnknownObject)
        );
        assert_eq!(
            first.next.apply_controller(1, first.after.id(), OTHER),
            Err(ObjectIdentityError::NoController)
        );
        assert_eq!(first.next, saved);
        let mut exhausted = first.next.clone();
        exhausted.revision = u64::MAX;
        assert_eq!(
            exhausted.apply_zone_move(u64::MAX, first.after.id(), ObjectDestination::Exile),
            Err(ObjectIdentityError::RevisionExhausted)
        );
        assert_eq!(
            exhausted.apply_controller(u64::MAX, first.after.id(), OTHER),
            Err(ObjectIdentityError::RevisionExhausted)
        );
        exhausted = first.next.clone();
        exhausted.next_identity = u64::MAX;
        let copy = exhausted.clone();
        assert_eq!(
            exhausted.apply_zone_move(1, first.after.id(), ObjectDestination::Exile),
            Err(ObjectIdentityError::IdentityExhausted)
        );
        assert_eq!(
            exhausted.seed_card(1, CardId(12), OWNER, ObjectDestination::Hand),
            Err(ObjectIdentityError::IdentityExhausted)
        );
        assert_eq!(exhausted, copy);
    }
    #[test]
    fn controlled_zone_changes_keep_identity_and_uncontrolled_zones_reject_control() {
        for placement in [
            ObjectDestination::Battlefield { controller: OWNER },
            ObjectDestination::Stack { controller: OWNER },
        ] {
            let start = seeded(placement);
            for player in 0..6 {
                let change = start
                    .next
                    .apply_controller(1, start.after.id(), SeatId(player))
                    .unwrap();
                assert_eq!(change.after.id(), start.after.id());
                assert_eq!(change.after.zone(), start.after.zone());
                assert_eq!(change.after.owner(), OWNER);
                assert_eq!(change.after.controller(), Some(SeatId(player)));
            }
        }
        for placement in [
            ObjectDestination::LibraryBottom,
            ObjectDestination::Hand,
            ObjectDestination::Graveyard,
            ObjectDestination::Exile,
            ObjectDestination::Command,
        ] {
            let start = seeded(placement);
            assert_eq!(
                start.next.apply_controller(1, start.after.id(), OTHER),
                Err(ObjectIdentityError::NoController)
            );
        }
    }
}
