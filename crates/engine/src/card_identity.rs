//! Ordinary nontoken-card identity bookkeeping (CR 108.3, 108.4, 400.3, 400.7–400.10).
//!
//! Trusted kernel continuations only: callers first resolve legality, replacements and
//! any information retained by a CR 400.7 exception. This module does not implement
//! those rules, ordered zones, hidden-information projections, tokens or planar ownership.
use crate::priority::SeatId;
use std::collections::BTreeMap;

/// One card supplied to this game, not a catalogue ID, collection grant or rules target.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct CardId(pub u64);

/// One incarnation of a card within one registry/game. The enclosing host binds
/// handles to that game and serializes competing successors. Within the accepted
/// state, a stale incarnation cannot address a returned card.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct CardObjectId(u64);

/// Individual players have distinct libraries, hands and graveyards.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CardZone {
    Library(SeatId),
    Hand(SeatId),
    Graveyard(SeatId),
    Battlefield,
    Stack,
    Exile,
    Command,
}

/// Already-established placement. Owned zones always resolve to this card's owner
/// (CR 400.3); a controller must be supplied for a spell or permanent (CR 108.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CardDestination {
    Library,
    Hand,
    Graveyard,
    Battlefield { controller: SeatId },
    Stack { controller: SeatId },
    Exile,
    Command,
}
impl CardDestination {
    fn placement(self, owner: SeatId) -> (CardZone, Option<SeatId>) {
        match self {
            Self::Library => (CardZone::Library(owner), None),
            Self::Hand => (CardZone::Hand(owner), None),
            Self::Graveyard => (CardZone::Graveyard(owner), None),
            Self::Battlefield { controller } => (CardZone::Battlefield, Some(controller)),
            Self::Stack { controller } => (CardZone::Stack, Some(controller)),
            Self::Exile => (CardZone::Exile, None),
            Self::Command => (CardZone::Command, None),
        }
    }
}

/// Minimal identity/location snapshot, deliberately not a complete Magic object.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CardObject {
    id: CardObjectId,
    card: CardId,
    owner: SeatId,
    zone: CardZone,
    controller: Option<SeatId>,
    tapped: Option<bool>,
}
impl CardObject {
    pub fn id(&self) -> CardObjectId {
        self.id
    }
    pub fn card(&self) -> CardId {
        self.card
    }
    pub fn owner(&self) -> SeatId {
        self.owner
    }
    pub fn zone(&self) -> CardZone {
        self.zone
    }
    pub fn controller(&self) -> Option<SeatId> {
        self.controller
    }
    /// Physical status exists only on the battlefield (CR 110.5).
    pub fn tapped(&self) -> Option<bool> {
        self.tapped
    }
}

/// Immutable native state for ordinary cards; not a public API or durable commit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CardObjects {
    revision: u64,
    next_identity: u64,
    current: BTreeMap<CardObjectId, CardObject>,
    by_card: BTreeMap<CardId, CardObjectId>,
}

/// Internal evidence for a later authoritative event; never a seat-safe projection.
/// The prior snapshot is retained for the kernel's last-known-information processing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CardChange {
    pub next: CardObjects,
    pub before: Option<CardObject>,
    pub after: CardObject,
}

/// Rejections and operational exhaustion do not choose a game result.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CardIdentityError {
    StaleRevision,
    RevisionExhausted,
    IdentityExhausted,
    CardAlreadyPresent,
    UnknownObject,
    SameZone,
    NoController,
    NotPermanent,
    StatusUnchanged,
}

impl CardObjects {
    pub fn new(revision: u64) -> Self {
        Self {
            revision,
            next_identity: 1,
            current: BTreeMap::new(),
            by_card: BTreeMap::new(),
        }
    }
    pub fn revision(&self) -> u64 {
        self.revision
    }
    pub fn get(&self, id: CardObjectId) -> Option<&CardObject> {
        self.current.get(&id)
    }
    /// Canonical native objects; callers must project these before any client exposure.
    pub fn iter(&self) -> impl Iterator<Item = &CardObject> {
        self.current.values()
    }
    /// Administrative native lookup only. Rules targets must retain CardObjectId,
    /// never follow this stable bookkeeping key through a zone change.
    pub fn current_identity(&self, card: CardId) -> Option<CardObjectId> {
        self.by_card.get(&card).copied()
    }
    fn next_revision(&self, expected: u64) -> Result<u64, CardIdentityError> {
        if expected != self.revision {
            return Err(CardIdentityError::StaleRevision);
        }
        self.revision
            .checked_add(1)
            .ok_or(CardIdentityError::RevisionExhausted)
    }
    fn allocate_identity(&self) -> Result<(CardObjectId, u64), CardIdentityError> {
        let next = self
            .next_identity
            .checked_add(1)
            .ok_or(CardIdentityError::IdentityExhausted)?;
        Ok((CardObjectId(self.next_identity), next))
    }
    /// Trusted setup after the enclosing kernel establishes an ordinary card's owner
    /// and placement. No deck legality, participant validation or ownership grant occurs.
    pub fn seed_card(
        &self,
        expected: u64,
        card: CardId,
        owner: SeatId,
        destination: CardDestination,
    ) -> Result<CardChange, CardIdentityError> {
        let revision = self.next_revision(expected)?;
        if self.by_card.contains_key(&card) {
            return Err(CardIdentityError::CardAlreadyPresent);
        }
        let (id, next_identity) = self.allocate_identity()?;
        let (zone, controller) = destination.placement(owner);
        let after = CardObject {
            id,
            card,
            owner,
            zone,
            controller,
            tapped: (zone == CardZone::Battlefield).then_some(false),
        };
        let mut next = self.clone();
        next.revision = revision;
        next.next_identity = next_identity;
        next.current.insert(id, after.clone());
        next.by_card.insert(card, id);
        Ok(CardChange {
            next,
            before: None,
            after,
        })
    }
    /// Apply an otherwise legal, replacement-adjusted move. Every zone change gets
    /// a fresh incarnation. Reexiling and putting a card into the command zone again
    /// also refresh identity (CR 400.8/400.10). Same-zone library/hand/graveyard
    /// rearrangements are outside this operation; they must not renew identity.
    /// CR 400.9 command-zone face changes require later face-state processing.
    pub fn apply_zone_move(
        &self,
        expected: u64,
        object: CardObjectId,
        destination: CardDestination,
    ) -> Result<CardChange, CardIdentityError> {
        let revision = self.next_revision(expected)?;
        let before = self.get(object).ok_or(CardIdentityError::UnknownObject)?;
        let (zone, controller) = destination.placement(before.owner);
        if zone == before.zone && !matches!(zone, CardZone::Exile | CardZone::Command) {
            return Err(CardIdentityError::SameZone);
        }
        let (id, next_identity) = self.allocate_identity()?;
        let after = CardObject {
            id,
            card: before.card,
            owner: before.owner,
            zone,
            controller,
            tapped: (zone == CardZone::Battlefield).then_some(false),
        };
        let mut next = self.clone();
        next.revision = revision;
        next.next_identity = next_identity;
        next.current.remove(&object);
        next.current.insert(id, after.clone());
        next.by_card.insert(before.card, id);
        Ok(CardChange {
            next,
            before: Some(before.clone()),
            after,
        })
    }
    /// Trusted completed status change, preserving incarnation. The enclosing
    /// kernel establishes effect/cost legality; this is not a player action.
    pub fn set_tapped(
        &self,
        expected: u64,
        object: CardObjectId,
        tapped: bool,
    ) -> Result<CardChange, CardIdentityError> {
        let revision = self.next_revision(expected)?;
        let before = self.get(object).ok_or(CardIdentityError::UnknownObject)?;
        let prior = before.tapped.ok_or(CardIdentityError::NotPermanent)?;
        if prior == tapped {
            return Err(CardIdentityError::StatusUnchanged);
        }
        let after = CardObject {
            tapped: Some(tapped),
            ..before.clone()
        };
        let mut next = self.clone();
        next.revision = revision;
        next.current.insert(object, after.clone());
        Ok(CardChange {
            next,
            before: Some(before.clone()),
            after,
        })
    }
    #[cfg(test)]
    pub(crate) fn set_test_revision(&mut self, revision: u64) {
        self.revision = revision;
    }
    /// Record a controller already computed by the kernel, preserving incarnation.
    /// Does not calculate control layers, trigger events or authorize a player action.
    pub fn apply_controller(
        &self,
        expected: u64,
        object: CardObjectId,
        controller: SeatId,
    ) -> Result<CardChange, CardIdentityError> {
        let revision = self.next_revision(expected)?;
        let before = self.get(object).ok_or(CardIdentityError::UnknownObject)?;
        if !matches!(before.zone, CardZone::Battlefield | CardZone::Stack) {
            return Err(CardIdentityError::NoController);
        }
        let after = CardObject {
            controller: Some(controller),
            ..before.clone()
        };
        let mut next = self.clone();
        next.revision = revision;
        next.current.insert(object, after.clone());
        Ok(CardChange {
            next,
            before: Some(before.clone()),
            after,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn status_revision_failure_preserves_physical_status_and_incarnation() {
        let mut state = CardObjects::new(0)
            .seed_card(
                0,
                CardId(99),
                SeatId(0),
                CardDestination::Battlefield {
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
            Err(CardIdentityError::RevisionExhausted)
        );
        assert_eq!(state, before);
        assert_eq!(state.get(object).unwrap().tapped(), Some(false));
    }
    const OWNER: SeatId = SeatId(0);
    const OTHER: SeatId = SeatId(1);
    fn seeded(destination: CardDestination) -> CardChange {
        CardObjects::new(0)
            .seed_card(0, CardId(11), OWNER, destination)
            .unwrap()
    }
    #[test]
    fn a_stolen_card_returns_as_a_new_object_and_does_not_keep_its_controller() {
        let first = seeded(CardDestination::Battlefield { controller: OWNER });
        let original = first.after.id();
        let stolen = first.next.apply_controller(1, original, OTHER).unwrap();
        assert_eq!(stolen.after.id(), original);
        assert_eq!(stolen.after.owner(), OWNER);
        assert_eq!(stolen.after.controller(), Some(OTHER));
        let hand = stolen
            .next
            .apply_zone_move(2, original, CardDestination::Hand)
            .unwrap();
        assert_eq!(hand.before.as_ref().unwrap().controller(), Some(OTHER));
        assert_eq!(hand.after.zone(), CardZone::Hand(OWNER));
        assert_eq!(hand.after.controller(), None);
        assert!(hand.next.get(original).is_none());
        let returned = hand
            .next
            .apply_zone_move(
                3,
                hand.after.id(),
                CardDestination::Battlefield { controller: OWNER },
            )
            .unwrap();
        assert_ne!(original, returned.after.id());
        assert_ne!(hand.after.id(), returned.after.id());
        assert_eq!(returned.after.card(), CardId(11));
        assert_eq!(returned.after.controller(), Some(OWNER));
        assert_eq!(
            returned.next.apply_controller(4, original, OTHER),
            Err(CardIdentityError::UnknownObject)
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
            (CardDestination::Library, CardZone::Library(OWNER), None),
            (CardDestination::Hand, CardZone::Hand(OWNER), None),
            (CardDestination::Graveyard, CardZone::Graveyard(OWNER), None),
            (
                CardDestination::Battlefield { controller: OTHER },
                CardZone::Battlefield,
                Some(OTHER),
            ),
            (
                CardDestination::Stack { controller: OTHER },
                CardZone::Stack,
                Some(OTHER),
            ),
            (CardDestination::Exile, CardZone::Exile, None),
            (CardDestination::Command, CardZone::Command, None),
        ];
        for (source, source_zone, source_controller) in places {
            for (destination, zone, controller) in places {
                let start = seeded(source);
                assert_eq!(start.after.zone(), source_zone);
                assert_eq!(start.after.controller(), source_controller);
                let saved = start.next.clone();
                let result = start.next.apply_zone_move(1, start.after.id(), destination);
                if zone == start.after.zone()
                    && !matches!(zone, CardZone::Exile | CardZone::Command)
                {
                    assert_eq!(result, Err(CardIdentityError::SameZone));
                } else {
                    let change = result.unwrap();
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
        let first = seeded(CardDestination::Hand);
        let second = first
            .next
            .seed_card(1, CardId(12), OWNER, CardDestination::Hand)
            .unwrap();
        assert_ne!(first.after.id(), second.after.id());
        assert_eq!(second.next.get(first.after.id()), Some(&first.after));
        assert_eq!(
            second
                .next
                .seed_card(2, CardId(11), OWNER, CardDestination::Exile),
            Err(CardIdentityError::CardAlreadyPresent)
        );
    }
    #[test]
    fn stale_invalid_and_exhausted_transitions_preserve_input() {
        let first = seeded(CardDestination::Hand);
        let saved = first.next.clone();
        assert_eq!(
            first
                .next
                .apply_zone_move(0, first.after.id(), CardDestination::Exile),
            Err(CardIdentityError::StaleRevision)
        );
        assert_eq!(
            first
                .next
                .apply_zone_move(1, CardObjectId(999), CardDestination::Exile),
            Err(CardIdentityError::UnknownObject)
        );
        assert_eq!(
            first.next.apply_controller(1, first.after.id(), OTHER),
            Err(CardIdentityError::NoController)
        );
        assert_eq!(first.next, saved);
        let mut exhausted = first.next.clone();
        exhausted.revision = u64::MAX;
        assert_eq!(
            exhausted.apply_zone_move(u64::MAX, first.after.id(), CardDestination::Exile),
            Err(CardIdentityError::RevisionExhausted)
        );
        assert_eq!(
            exhausted.apply_controller(u64::MAX, first.after.id(), OTHER),
            Err(CardIdentityError::RevisionExhausted)
        );
        exhausted = first.next.clone();
        exhausted.next_identity = u64::MAX;
        let copy = exhausted.clone();
        assert_eq!(
            exhausted.apply_zone_move(1, first.after.id(), CardDestination::Exile),
            Err(CardIdentityError::IdentityExhausted)
        );
        assert_eq!(
            exhausted.seed_card(1, CardId(12), OWNER, CardDestination::Hand),
            Err(CardIdentityError::IdentityExhausted)
        );
        assert_eq!(exhausted, copy);
    }
    #[test]
    fn controlled_zone_changes_keep_identity_and_uncontrolled_zones_reject_control() {
        for placement in [
            CardDestination::Battlefield { controller: OWNER },
            CardDestination::Stack { controller: OWNER },
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
            CardDestination::Library,
            CardDestination::Hand,
            CardDestination::Graveyard,
            CardDestination::Exile,
            CardDestination::Command,
        ] {
            let start = seeded(placement);
            assert_eq!(
                start.next.apply_controller(1, start.after.id(), OTHER),
                Err(CardIdentityError::NoController)
            );
        }
    }
}
