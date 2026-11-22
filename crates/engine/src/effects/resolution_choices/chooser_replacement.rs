//! A departed player's choice belongs to the same suspended instruction.
use super::*;

/// A live player must choose someone to answer a departed player's effect choice.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReplacementChooserFrame {
    revision: u64,
    entry: StackEntryId,
    instruction: usize,
    departed: SeatId,
    chooser: SeatId,
    candidates: Vec<SeatId>,
}

impl ReplacementChooserFrame {
    /// The suspended instruction's resolving object.
    pub fn entry(&self) -> StackEntryId {
        self.entry
    }

    /// The exact suspended instruction.
    pub fn instruction(&self) -> usize {
        self.instruction
    }

    /// The seat that can no longer answer the original choice.
    pub fn departed(&self) -> SeatId {
        self.departed
    }

    /// The player authorized to select a replacement.
    pub fn chooser(&self) -> SeatId {
        self.chooser
    }

    /// Current eligible replacements in participant order.
    pub fn candidates(&self) -> &[SeatId] {
        &self.candidates
    }
}

impl EffectWindow {
    pub(super) fn replacement_chooser_frame(
        &self,
        work: &OptionalEffectWork,
    ) -> Option<ReplacementChooserFrame> {
        if self.window.participants().contains(&work.chooser) {
            return None;
        }
        let original_role = match self.optional_instruction(&work.cursor).ok()? {
            Instruction::OptionalEffect { chooser }
            | Instruction::OptionalSequence { chooser, .. } => *chooser,
            _ => return None,
        };
        let entry = self
            .window
            .resolving()
            .filter(|entry| entry.id() == work.cursor.entry)?;
        let controller = entry.controller();
        // The rule assigns the election itself to the next player if its usual chooser left.
        let chooser = if self.window.participants().contains(&controller) {
            controller
        } else {
            self.window.next_participant_after(controller).ok()?
        };
        let prefer_opponent =
            original_role != ResolvingPlayer::Controller && work.chooser != controller;
        let mut candidates = self
            .window
            .participants()
            .iter()
            .copied()
            .filter(|seat| !prefer_opponent || *seat != controller)
            .collect::<Vec<_>>();
        if candidates.is_empty() {
            candidates.extend_from_slice(self.window.participants());
        }
        Some(ReplacementChooserFrame {
            revision: self.revision,
            entry: work.cursor.entry,
            instruction: work.cursor.instruction,
            departed: work.chooser,
            chooser,
            candidates,
        })
    }

    pub(in crate::effects) fn resume_replacement_chooser(
        &self,
        expected: u64,
        frame: &ReplacementChooserFrame,
        actor: SeatId,
        reply: DecisionReply,
    ) -> Result<RuleProgress, EffectError> {
        let revision = self.next_revision(expected)?;
        if self.pending_resolution_decision()
            != Some(EffectDecision::ReplacementChooser(frame.clone()))
        {
            return Err(EffectError::WrongOptionalEffectFrame);
        }
        if actor != frame.chooser {
            return Err(EffectError::WrongChooser);
        }
        let DecisionReply::ReplacementChooser(selected) = reply else {
            return Err(EffectError::WrongDecisionKind);
        };
        if !frame.candidates.contains(&selected) {
            return Err(EffectError::SelectionOutsideDomain);
        }
        let mut next = self.clone();
        let Some(PendingWork::OptionalEffect(work)) = next.pending_work.as_mut() else {
            return Err(EffectError::WrongOptionalEffectFrame);
        };
        work.chooser = selected;
        next.revision = revision;
        let append = RulesJournal {
            records: vec![crate::occurrence::RulesRecord::ChooserReplaced {
                entry: frame.entry,
                instruction: frame.instruction,
                departed: frame.departed,
                chooser: actor,
                selected,
            }],
        };
        let frame = next
            .pending_resolution_decision()
            .ok_or(EffectError::InconsistentProgram)?;
        Ok(RuleProgress::new(
            next,
            RuleResult::Pending {
                checks: CheckJournal::default(),
                frame,
                journal: DamageJournal::default(),
                declarations: vec![],
                assignments: vec![],
                draws: DrawJournal::default(),
            },
            append,
        ))
    }
}
