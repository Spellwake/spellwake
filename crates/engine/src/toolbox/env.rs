//! Typed resolution binding frame (`ExecEnv`) and LKI snapshots.
//!
//! Replaces ad-hoc resolution cursor fields with an orthogonal slot map
//! (`BindingId -> ExecValue`) that chains across instructions, loops,
//! reflexive triggers, and delayed triggers.

use crate::{
    effects::CounterKind,
    mana::ManaType,
    object_identity::{CardId, ObjectId, ObjectZone},
    priority::SeatId,
    program::{CardType, ColorMask},
};
use spellwake_contracts::{RulesInteger, creature_types::CreatureTypeId};
use std::collections::{BTreeMap, BTreeSet};

/// Compact slot identifier inside an [`ExecEnv`] frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct BindingId(pub u16);

impl BindingId {
    /// Reserved slot synced with `ResolutionResults.loop_player` for `ForEachPlayerQuery`.
    pub const LOOP_PLAYER: Self = Self(u16::MAX);
}

/// Last-known-information snapshot captured before an object changes zones (CR 400.7, 608.2h).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObjectSnapshot {
    pub object: ObjectId,
    pub card: Option<CardId>,
    pub owner: SeatId,
    pub controller: SeatId,
    pub zone: ObjectZone,
    pub power: Option<RulesInteger>,
    pub toughness: Option<RulesInteger>,
    pub mana_value: RulesInteger,
    pub card_types: BTreeSet<CardType>,
    pub colors: ColorMask,
    pub subtypes: BTreeSet<CreatureTypeId>,
    pub counters: BTreeMap<CounterKind, RulesInteger>,
}

/// Typed runtime value stored in [`ExecEnv`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExecValue {
    Objects(Vec<ObjectId>),
    ObjectSnapshots(Vec<ObjectSnapshot>),
    Players(Vec<SeatId>),
    Int(RulesInteger),
    Bool(bool),
    Colors(ColorMask),
    CardTypes(BTreeSet<CardType>),
    Subtypes(BTreeSet<CreatureTypeId>),
    CounterKind(CounterKind),
    CardName(String),
    Mana(Vec<ManaType>),
    Piles(Vec<Vec<ObjectId>>),
    OptionIndex(u8),
}

/// Deterministic resolution binding frame mapping [`BindingId`] slots to typed values.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ExecEnv {
    slots: BTreeMap<BindingId, ExecValue>,
}

impl ExecEnv {
    /// Create an empty binding frame.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            slots: BTreeMap::new(),
        }
    }

    /// Bind `value` into `id`, overwriting any previous binding in that slot.
    pub fn bind(&mut self, id: BindingId, value: ExecValue) {
        self.slots.insert(id, value);
    }

    /// Read the raw [`ExecValue`] stored in `id`.
    #[must_use]
    pub fn get(&self, id: BindingId) -> Option<&ExecValue> {
        self.slots.get(&id)
    }

    /// Return whether `id` is currently bound.
    #[must_use]
    pub fn contains(&self, id: BindingId) -> bool {
        self.slots.contains_key(&id)
    }

    /// Return whether no bindings are present.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.slots.is_empty()
    }

    /// Read bound object IDs from `id`, or an empty slice if absent or another variant.
    #[must_use]
    pub fn objects(&self, id: BindingId) -> &[ObjectId] {
        match self.slots.get(&id) {
            Some(ExecValue::Objects(ids)) => ids.as_slice(),
            _ => &[],
        }
    }

    /// Read bound LKI snapshots from `id`.
    #[must_use]
    pub fn snapshots(&self, id: BindingId) -> &[ObjectSnapshot] {
        match self.slots.get(&id) {
            Some(ExecValue::ObjectSnapshots(snaps)) => snaps.as_slice(),
            _ => &[],
        }
    }

    /// Read bound player seats from `id`.
    #[must_use]
    pub fn players(&self, id: BindingId) -> &[SeatId] {
        match self.slots.get(&id) {
            Some(ExecValue::Players(seats)) => seats.as_slice(),
            _ => &[],
        }
    }

    /// Read a bound integer from `id`.
    #[must_use]
    pub fn int(&self, id: BindingId) -> Option<&RulesInteger> {
        match self.slots.get(&id) {
            Some(ExecValue::Int(v)) => Some(v),
            _ => None,
        }
    }

    /// Read a bound boolean from `id`.
    #[must_use]
    pub fn bool_flag(&self, id: BindingId) -> Option<bool> {
        match self.slots.get(&id) {
            Some(ExecValue::Bool(b)) => Some(*b),
            _ => None,
        }
    }

    /// Read bound color mask from `id`.
    #[must_use]
    pub fn colors(&self, id: BindingId) -> Option<ColorMask> {
        match self.slots.get(&id) {
            Some(ExecValue::Colors(mask)) => Some(*mask),
            _ => None,
        }
    }

    /// Read bound card types from `id`.
    #[must_use]
    pub fn card_types(&self, id: BindingId) -> Option<&BTreeSet<CardType>> {
        match self.slots.get(&id) {
            Some(ExecValue::CardTypes(types)) => Some(types),
            _ => None,
        }
    }

    /// Read bound creature subtypes from `id`.
    #[must_use]
    pub fn subtypes(&self, id: BindingId) -> Option<&BTreeSet<CreatureTypeId>> {
        match self.slots.get(&id) {
            Some(ExecValue::Subtypes(subs)) => Some(subs),
            _ => None,
        }
    }

    /// Read bound counter kind from `id`.
    #[must_use]
    pub fn counter_kind(&self, id: BindingId) -> Option<CounterKind> {
        match self.slots.get(&id) {
            Some(ExecValue::CounterKind(kind)) => Some(*kind),
            _ => None,
        }
    }

    /// Read bound card name from `id`.
    #[must_use]
    pub fn card_name(&self, id: BindingId) -> Option<&str> {
        match self.slots.get(&id) {
            Some(ExecValue::CardName(name)) => Some(name.as_str()),
            _ => None,
        }
    }

    /// Read bound piles from `id`.
    #[must_use]
    pub fn piles(&self, id: BindingId) -> Option<&[Vec<ObjectId>]> {
        match self.slots.get(&id) {
            Some(ExecValue::Piles(piles)) => Some(piles.as_slice()),
            _ => None,
        }
    }

    /// Read bound option index from `id`.
    #[must_use]
    pub fn option_index(&self, id: BindingId) -> Option<u8> {
        match self.slots.get(&id) {
            Some(ExecValue::OptionIndex(idx)) => Some(*idx),
            _ => None,
        }
    }

    /// Merge all bindings from `other` into `self` (overwriting colliding slots).
    pub fn merge_from(&mut self, other: &Self) {
        for (k, v) in &other.slots {
            self.slots.insert(*k, v.clone());
        }
    }

    /// Iterate over all bound entries in deterministic [`BindingId`] order.
    pub fn iter(&self) -> impl Iterator<Item = (BindingId, &ExecValue)> {
        self.slots.iter().map(|(k, v)| (*k, v))
    }
}
