//! Generic object metadata tags/partitions, state tracks, and side-decks.
//!
//! Unifies mechanics that attach metadata or groupings to objects (Space Beleren
//! sectors, Raging River piles, Fact or Fiction splits, Stickers, Attractions,
//! Contraptions, linked exiles), track game/player/object progression (Speed,
//! Day/Night, Monarch, Initiative, Dungeon rooms, Class levels), or draw from
//! supplemental decks (Planar, Attraction, Contraption, Dungeon).

use crate::{
    object_identity::{CardId, ObjectId},
    priority::SeatId,
    program::ValueExpr,
    toolbox::{
        env::BindingId,
        query::{ObjectQuery, PlayerSelector},
    },
};
use spellwake_contracts::RulesInteger;
use std::collections::BTreeMap;

/// Key namespace for generic object metadata tags and partitions.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum TagKey {
    /// Named partition/sector/pile index (e.g. battlefield sectors, pile splits).
    Partition(u16),
    /// General string-keyed metadata tag (e.g. stickers, attraction numbers, custom markers).
    Label(String),
    /// Association with another game object (e.g. linked exile, imprinted card).
    LinkedSource(ObjectId),
}

/// Typed value attached to an object under a [`TagKey`].
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum TagValue {
    Unit,
    Int(RulesInteger),
    Seat(SeatId),
    Object(ObjectId),
    Text(String),
    Flag(bool),
}

/// Deterministic store of generic metadata tags and partition memberships on game objects.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ObjectTagStore {
    tags: BTreeMap<ObjectId, BTreeMap<TagKey, TagValue>>,
}

impl ObjectTagStore {
    /// Set `key = value` on `object`.
    pub fn set(&mut self, object: ObjectId, key: TagKey, value: TagValue) {
        self.tags.entry(object).or_default().insert(key, value);
    }

    /// Remove `key` from `object`.
    pub fn remove(&mut self, object: ObjectId, key: &TagKey) -> Option<TagValue> {
        let map = self.tags.get_mut(&object)?;
        let removed = map.remove(key);
        if map.is_empty() {
            self.tags.remove(&object);
        }
        removed
    }

    /// Clear all tags attached to `object`.
    pub fn clear_object(&mut self, object: ObjectId) {
        self.tags.remove(&object);
    }

    /// Read the value of `key` on `object`.
    #[must_use]
    pub fn get(&self, object: ObjectId, key: &TagKey) -> Option<&TagValue> {
        self.tags.get(&object)?.get(key)
    }

    /// Check whether `object` has `key`, optionally matching a specific `value`.
    #[must_use]
    pub fn has(&self, object: ObjectId, key: &TagKey, expected: Option<&TagValue>) -> bool {
        match self.get(object, key) {
            Some(actual) => expected.is_none_or(|want| actual == want),
            None => false,
        }
    }

    /// Return all objects in deterministic order having `key` (and matching `expected` when given).
    #[must_use]
    pub fn objects_with_tag(&self, key: &TagKey, expected: Option<&TagValue>) -> Vec<ObjectId> {
        self.tags
            .iter()
            .filter_map(|(id, map)| {
                let actual = map.get(key)?;
                expected.is_none_or(|want| actual == want).then_some(*id)
            })
            .collect()
    }

    /// Return all objects belonging to `partition_id` with pile/sector index `index`.
    #[must_use]
    pub fn partition_members(&self, partition_id: u16, index: usize) -> Vec<ObjectId> {
        let key = TagKey::Partition(partition_id);
        let value = TagValue::Int(RulesInteger::from_usize(index));
        self.objects_with_tag(&key, Some(&value))
    }
}

/// Resolved scope key inside [`StateTrackStore`].
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum StateTrackKey {
    /// Game-wide track (e.g. day/night, monarch, initiative).
    Global(String),
    /// Per-player track (e.g. speed, city's blessing, ring level, dungeon room, tickets).
    Player(SeatId, String),
    /// Per-object track (e.g. class level, station charge, room door state).
    Object(ObjectId, String),
}

/// Program-time scope selector resolved to a [`StateTrackKey`] at execution time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StateTrackScope {
    Global,
    Player(Box<PlayerSelector>),
    Object(Box<ObjectQuery>),
}

/// Program-time specification of a state track key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StateTrackKeySpec {
    pub scope: StateTrackScope,
    pub name: String,
}

/// Typed value held by a state track.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StateTrackValue {
    Int(RulesInteger),
    Seat(SeatId),
    Bool(bool),
    Label(String),
}

/// Mutation applied by `Instruction::UpdateStateTrack`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StateTrackOp {
    SetInt(Box<ValueExpr>),
    AddInt(Box<ValueExpr>),
    SetBool(bool),
    SetSeat(Box<PlayerSelector>),
    SetLabel(String),
    Clear,
}

/// Deterministic store of generic global, per-player, and per-object state tracks.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StateTrackStore {
    tracks: BTreeMap<StateTrackKey, StateTrackValue>,
}

impl StateTrackStore {
    /// Read a track value.
    #[must_use]
    pub fn get(&self, key: &StateTrackKey) -> Option<&StateTrackValue> {
        self.tracks.get(key)
    }

    /// Read a numeric track value, defaulting to zero when unset or non-numeric.
    #[must_use]
    pub fn int_or_zero(&self, key: &StateTrackKey) -> RulesInteger {
        match self.tracks.get(key) {
            Some(StateTrackValue::Int(v)) => v.clone(),
            _ => RulesInteger::from(0),
        }
    }

    /// Set a track value.
    pub fn set(&mut self, key: StateTrackKey, value: StateTrackValue) {
        self.tracks.insert(key, value);
    }

    /// Increment (or initialize) a numeric track by `delta` and return the new value.
    pub fn add_int(&mut self, key: StateTrackKey, delta: &RulesInteger) -> RulesInteger {
        let next = self.int_or_zero(&key).plus(delta);
        self.tracks.insert(key, StateTrackValue::Int(next.clone()));
        next
    }

    /// Remove a track entry.
    pub fn clear(&mut self, key: &StateTrackKey) -> Option<StateTrackValue> {
        self.tracks.remove(key)
    }
}

/// Category of supplemental side-deck.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum SideDeckKind {
    Planar,
    Attraction,
    Contraption,
    Dungeon,
    Custom(u16),
}

/// Deterministic supplemental deck owned by a player or shared globally (`None`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SideDeck {
    /// Ordered top-to-bottom remaining cards/nodes.
    pub cards: Vec<CardId>,
    /// Currently active face-up card/node (e.g. current plane or active dungeon).
    pub active: Option<CardId>,
    /// Discard / junkyard pile.
    pub discard: Vec<CardId>,
}

impl SideDeck {
    /// Reveal/draw the top card, making it active and moving any previous active card to discard.
    pub fn advance_top(&mut self) -> Option<CardId> {
        if self.cards.is_empty() {
            return None;
        }
        let next = self.cards.remove(0);
        if let Some(prev) = self.active.replace(next) {
            self.discard.push(prev);
        }
        Some(next)
    }

    /// Put the current active card on the bottom of the deck and draw the next top card.
    pub fn cycle_to_bottom(&mut self) -> Option<CardId> {
        if let Some(prev) = self.active.take() {
            self.cards.push(prev);
        }
        if self.cards.is_empty() {
            return None;
        }
        let next = self.cards.remove(0);
        self.active = Some(next);
        Some(next)
    }

    /// Peek the top `count` cards without mutating the deck.
    #[must_use]
    pub fn peek_top(&self, count: usize) -> Vec<CardId> {
        self.cards.iter().take(count).copied().collect()
    }
}

/// Deterministic store of supplemental side-decks keyed by `(owner, kind)`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SideDeckStore {
    decks: BTreeMap<(Option<SeatId>, SideDeckKind), SideDeck>,
}

impl SideDeckStore {
    /// Install or replace a side-deck.
    pub fn insert(&mut self, owner: Option<SeatId>, kind: SideDeckKind, deck: SideDeck) {
        self.decks.insert((owner, kind), deck);
    }

    /// Borrow a side-deck.
    #[must_use]
    pub fn get(&self, owner: Option<SeatId>, kind: SideDeckKind) -> Option<&SideDeck> {
        self.decks.get(&(owner, kind))
    }

    /// Mutably borrow a side-deck, creating an empty one if absent.
    pub fn get_mut_or_default(
        &mut self,
        owner: Option<SeatId>,
        kind: SideDeckKind,
    ) -> &mut SideDeck {
        self.decks.entry((owner, kind)).or_default()
    }
}

/// Operation executed on a [`SideDeck`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SideDeckOpKind {
    /// Draw top card as active (discarding prior active).
    AdvanceTop,
    /// Put current active card on bottom and draw top card as active (e.g. planeswalk).
    CycleToBottom,
    /// Move all discarded cards back into the deck (ordered deterministically).
    RecycleDiscardToBottom,
}

/// Program specification for `Instruction::SideDeckOp`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SideDeckOpSpec {
    /// `None` addresses the shared global deck; `Some` addresses the first selected player's deck.
    pub owner: Option<PlayerSelector>,
    pub kind: SideDeckKind,
    pub op: SideDeckOpKind,
    /// Optional binding receiving `ExecValue::Bool(succeeded)`.
    pub bind_succeeded: Option<BindingId>,
}
