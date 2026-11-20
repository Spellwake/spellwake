//! Closed test-only source transformation, never a gameplay input or restore API.
use super::*;
use crate::test_support::related_player_fault::{
    RelatedPlayerCauseFault, RelatedPlayerFaultSetupError,
};

impl GameState {
    /// Clone a genuinely reached Controller/free gate with one disclosed cause fault.
    /// No rule transition, decision issuance, adoption or journal is produced.
    pub(crate) fn qualification_with_related_player_cause_fault(
        &self,
        fault: RelatedPlayerCauseFault,
    ) -> Result<Self, RelatedPlayerFaultSetupError> {
        use RelatedPlayerFaultSetupError::WrongGate;
        let pending = self.pending.as_ref().ok_or(WrongGate)?;
        let RuleFrame::ResolutionPayment(frame) = &pending.frame else {
            return Err(WrongGate);
        };
        if pending.basis != self.basis
            || pending.chooser != frame.payer()
            || self.execution.pending_resolution_decision()
                != Some(EffectDecision::ResolutionPayment(frame.clone()))
        {
            return Err(WrongGate);
        }
        let execution = self
            .execution
            .qualification_with_related_player_cause_fault(fault)?;
        let next = Self {
            execution,
            ..self.clone()
        };
        // All private root fields other than the separately checked executor are exact.
        let unchanged = Self {
            execution: self.execution.clone(),
            ..next.clone()
        };
        assert_eq!(&unchanged, self);
        Ok(next)
    }
}
