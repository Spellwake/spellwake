//! Physical CR 800.4a operations. The executor owns evaluation and publication.
use super::*;

pub(crate) struct OwnedDeparture {
    pub window: StackWindow,
    pub objects: Vec<GameObject>,
    pub entries: Vec<StackEntry>,
    pub copies: Vec<SpellCopyObject>,
}
pub(crate) struct ControlledDeparture {
    pub window: StackWindow,
    pub movements: Vec<ObjectMovement>,
    pub entries: Vec<StackEntry>,
    pub copies: Vec<SpellCopyObject>,
}

impl StackWindow {
    /// Immediate CR 800.4c continuation after a control effect ends. Legality
    /// and replacement ownership are checked by the enclosing executor.
    pub(crate) fn exile_departed_defaults(
        &self,
        objects: &[ObjectId],
    ) -> Result<(Self, Vec<ObjectMovement>), StackWindowError> {
        let revision = self.next_revision(self.revision)?;
        let mut next = self.clone();
        let mut movements = Vec::new();
        for object in objects {
            let (window, exit) = next.move_card(revision, *object, ObjectDestination::Exile)?;
            next = window;
            let StackExit::Object(movement) = exit else {
                return Err(StackWindowError::InvalidCheckMove);
            };
            movements.push(movement);
        }
        next.revision = revision;
        Ok((next, movements))
    }
    pub(crate) fn remove_departing_owned(
        &self,
        players: &BTreeSet<SeatId>,
    ) -> Result<OwnedDeparture, StackWindowError> {
        let revision = self.next_revision(self.revision)?;
        let removed = self
            .cards
            .remove_owned_objects(self.cards.revision(), players)?;
        let mut next = self.with_cards(revision, removed.next);
        let mut entries = Vec::new();
        next.stack.retain(|entry| {
            let retained = match entry.kind() {
                StackKind::CardSpell(object) => next.cards.get(*object).is_some(),
                StackKind::SpellCopy { owner, .. } => !players.contains(owner),
                StackKind::ActivatedAbility { .. } | StackKind::TriggeredAbility { .. } => true,
            };
            if !retained {
                entries.push(entry.clone());
            }
            retained
        });
        let mut copies = Vec::new();
        next.copies.retain(|_, copy| {
            if players.contains(&copy.owner) {
                copies.push(copy.clone());
                false
            } else {
                true
            }
        });
        Ok(OwnedDeparture {
            window: next,
            objects: removed.objects,
            entries,
            copies,
        })
    }

    pub(crate) fn finish_player_departure(
        &self,
        players: &BTreeSet<SeatId>,
        exiles: &[ObjectId],
    ) -> Result<ControlledDeparture, StackWindowError> {
        let revision = self.next_revision(self.revision)?;
        let mut next = self.clone();
        let mut entries = Vec::new();
        next.stack.retain(|entry| {
            let retained = matches!(entry.kind(), StackKind::CardSpell(_))
                || !players.contains(&entry.controller());
            if !retained {
                entries.push(entry.clone());
            }
            retained
        });
        let mut copies = Vec::new();
        next.copies.retain(|_, copy| {
            if copy.controller.is_some_and(|seat| players.contains(&seat)) {
                copies.push(copy.clone());
                false
            } else {
                true
            }
        });
        let mut movements = Vec::new();
        for object in exiles {
            let (window, exit) = next.move_card(revision, *object, ObjectDestination::Exile)?;
            next = window;
            let StackExit::Object(movement) = exit else {
                return Err(StackWindowError::InvalidCheckMove);
            };
            movements.push(movement);
        }
        let start = self
            .order
            .iter()
            .position(|seat| *seat == self.priority_start)
            .ok_or(StackWindowError::UnknownPlayer)?;
        next.priority_start = self
            .order
            .iter()
            .cycle()
            .skip(start)
            .take(self.order.len())
            .copied()
            .find(|seat| !players.contains(seat))
            .ok_or(StackWindowError::UnknownPlayer)?;
        next.order.retain(|seat| !players.contains(seat));
        next.departed.extend(players);
        next.status = match &self.status {
            Status::Opening if next.stack.is_empty() => {
                // Before the first turn there is no active turn to finish.
                next.active = next.priority_start;
                Status::Opening
            }
            Status::TurnWork if next.stack.is_empty() => Status::TurnWork,
            Status::Priority(round) => Status::Priority(round.remove_players(players)?),
            Status::Resolving { entry, .. } => {
                if next.stack.iter().any(|current| current.id() == entry.id()) {
                    self.status.clone()
                } else {
                    // Ceasing is not completing resolution. Prefix effects stay
                    // committed; the enclosing executor drops only this cursor.
                    Status::Priority(PriorityRound::new(
                        next.order.clone(),
                        next.priority_start,
                        0,
                    )?)
                }
            }
            _ => return Err(StackWindowError::NotResolving),
        };
        next.revision = revision;
        Ok(ControlledDeparture {
            window: next,
            movements,
            entries,
            copies,
        })
    }
}
