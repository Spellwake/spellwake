//! Trusted unit movements through the existing registry and window owners.
use super::*;

impl StackWindow {
    /// Explicit private preparation installs exactly the actual primitive result.
    /// No window operation occurred, so its revision and phase remain unchanged.
    pub(crate) fn qualification_controller_successor(
        &self,
        object: ObjectId,
        controller: SeatId,
    ) -> Result<(Self, crate::object_identity::ObjectChange), StackWindowError> {
        let change = self
            .cards
            .apply_controller(self.cards.revision(), object, controller)?;
        let mut next = self.clone();
        next.cards = change.next.clone();
        Ok((next, change))
    }
    pub(crate) fn qualification_set_tapped(
        &self,
        expected: u64,
        objects: &[ObjectId],
        tapped: bool,
    ) -> Result<(Self, Vec<ObjectStatusChange>), StackWindowError> {
        let revision = self.next_revision(expected)?;
        let (cards, changes) =
            self.cards
                .set_tapped_batch(self.cards.revision(), objects, tapped)?;
        Ok((self.with_cards(revision, cards), changes))
    }

    /// A private prepared-unit mutation. It cannot move a tracked stack object or
    /// announce one; ordinary casting/resolution keep their existing owners.
    pub(crate) fn qualification_zone_moves(
        &self,
        expected: u64,
        moves: &[(ObjectId, ObjectDestination)],
    ) -> Result<(Self, Vec<ObjectMovement>), StackWindowError> {
        let revision = self.next_revision(expected)?;
        for &(object, destination) in moves {
            let source = self
                .cards
                .get(object)
                .ok_or(ObjectIdentityError::UnknownObject)?;
            if source.zone() == ObjectZone::Stack
                || matches!(destination, ObjectDestination::Stack { .. })
            {
                return Err(StackWindowError::InvalidCheckMove);
            }
            if let ObjectDestination::Battlefield { controller } = destination {
                self.validate_player(controller)?;
            }
        }
        let (cards, movements) = self.cards.apply_zone_moves(self.cards.revision(), moves)?;
        let movements = movements
            .into_iter()
            .map(crate::object_identity::ZoneMoveOutcome::into_moved)
            .collect::<Result<Vec<_>, _>>()?;
        Ok((self.with_cards(revision, cards), movements))
    }
}

// The receipt is private and owns no instruction body. Qualification observes
// its actual window through this cfg(test)-only owning descendant, without a
// production getter or a second native state execution.
impl crate::test_support::current_profile::applicability::sealed::Sealed for OrderedTriggerReceipt {}
impl crate::test_support::current_profile::applicability::NativeCaptureValue
    for OrderedTriggerReceipt
{
    fn check_current10(
        &self,
    ) -> Result<(), crate::test_support::current_profile::applicability::ApplicabilityError> {
        crate::test_support::current_profile::applicability::NativeCaptureValue::check_current10(
            &self.window,
        )
    }
}
