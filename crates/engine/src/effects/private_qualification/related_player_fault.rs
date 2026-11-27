//! Narrow invariant violation at the retained cause owner, absent in production.
use super::*;
use crate::{
    occurrence::TriggerCauseKey,
    program::ResolvingPlayer,
    test_support::related_player_fault::{RelatedPlayerCauseFault, RelatedPlayerFaultSetupError},
};

impl EffectWindow {
    pub(crate) fn qualification_with_related_player_cause_fault(
        &self,
        fault: RelatedPlayerCauseFault,
    ) -> Result<Self, RelatedPlayerFaultSetupError> {
        use RelatedPlayerFaultSetupError::*;
        let Some(PendingWork::ResolutionPayment(work)) = &self.pending_work else {
            return Err(WrongGate);
        };
        let entry = self.window.resolving().ok_or(WrongGate)?;
        let payload = match self.payloads.get(&entry.id()) {
            Some(StackPayload::Instructions(value)) => value,
            _ => return Err(WrongGate),
        };
        if work.cursor.entry != entry.id()
            || work.cursor.instruction != 0
            || work.payer != entry.controller()
            || !matches!(payload.body.instructions.first(), Some(Instruction::OfferDynamicManaPayment {
                payer: ResolvingPlayer::Controller, fixed: cost, ..
            }) if cost == &ManaCost::free())
            || !matches!(self.pending_resolution_decision(), Some(EffectDecision::ResolutionPayment(frame)) if frame.cost() == &ManaCost::free())
        {
            return Err(WrongGate);
        }
        let binding = payload.bindings.trigger.as_ref().ok_or(WrongBinding)?;
        if !matches!(entry.kind(), StackKind::TriggeredAbility { source } if source == &binding.source.object)
        {
            return Err(WrongBinding);
        }
        let original = binding.causes.members();
        if original.len() != 2 {
            return Err(InsufficientCauseMembers);
        }
        let Some(player) = original[0].related_player else {
            return Err(InvalidOriginalCauseSet);
        };
        let seats = self.window.participants();
        if binding.causes.key() != TriggerCauseKey::Player(player)
            || !seats.contains(&player)
            || original[1].related_player != Some(player)
            || original[0].occurrence == original[1].occurrence
            || original
                .iter()
                .any(|member| member.occurrence.event != binding.causes.event())
        {
            return Err(InvalidOriginalCauseSet);
        }
        if entry.controller() == player || !seats.contains(&entry.controller()) {
            return Err(ControllerNotDistinctParticipant);
        }
        let mut members = original.to_vec();
        match fault {
            RelatedPlayerCauseFault::EmptyCauseSet => members.clear(),
            RelatedPlayerCauseFault::MissingRelatedPlayer => members[0].related_player = None,
            RelatedPlayerCauseFault::ForeignParticipant => {
                let mut foreign = SeatId(0);
                while seats.contains(&foreign) {
                    foreign = SeatId(foreign.0.checked_add(1).ok_or(NoForeignSeat)?);
                }
                for member in &mut members {
                    member.related_player = Some(foreign);
                }
            }
            RelatedPlayerCauseFault::AmbiguousParticipants => {
                members[1].related_player = Some(entry.controller())
            }
        }
        let mut next = self.clone();
        let Some(StackPayload::Instructions(next_payload)) = next.payloads.get_mut(&entry.id())
        else {
            unreachable!("validated actual instruction payload");
        };
        next_payload
            .bindings
            .trigger
            .as_mut()
            .expect("validated actual retained trigger")
            .causes
            .set_test_members(members);
        // Compare every private executor field, restoring members ONLY in this assertion clone.
        let mut unchanged = next.clone();
        let Some(StackPayload::Instructions(check_payload)) =
            unchanged.payloads.get_mut(&entry.id())
        else {
            unreachable!();
        };
        check_payload
            .bindings
            .trigger
            .as_mut()
            .unwrap()
            .causes
            .set_test_members(original.to_vec());
        assert_eq!(&unchanged, self);
        Ok(next)
    }
}
