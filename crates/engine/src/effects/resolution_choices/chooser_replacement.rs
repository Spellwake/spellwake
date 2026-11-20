//! CR 800.4g substitution stays inside the existing optional cursor owner.
use super::*;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ChooserSubstitution {
    pub(super) controller: SeatId,
    pub(super) candidates: Vec<SeatId>,
}

/// Executor-issued selection by the surviving object's controller. This is
/// neither the absent player's optional response nor a rule/APNAP substitution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReplacementChooserFrame {
    pub(crate) revision: u64,
    entry: StackEntryId,
    instruction: usize,
    departed: SeatId,
    controller: SeatId,
    candidates: Vec<SeatId>,
}
impl ReplacementChooserFrame {
    /// Single-field malformed-token controls, absent from production builds.
    #[cfg(test)]
    pub(crate) fn qualification_mutations(
        &self,
        other_entry: StackEntryId,
    ) -> Vec<(&'static str, Self)> {
        let mut revision = self.clone();
        revision.revision = revision.revision.checked_add(1).unwrap();
        let mut entry = self.clone();
        entry.entry = other_entry;
        let mut instruction = self.clone();
        instruction.instruction = instruction.instruction.checked_add(1).unwrap();
        let mut departed = self.clone();
        departed.departed = self.controller;
        let mut controller = self.clone();
        controller.controller = self.departed;
        let mut candidates = self.clone();
        candidates.candidates.clear();
        vec![
            ("revision", revision),
            ("entry", entry),
            ("instruction", instruction),
            ("departed", departed),
            ("controller", controller),
            ("candidates", candidates),
        ]
    }
    /// Exact executor revision at which the replacement domain was issued.
    pub fn revision(&self) -> u64 {
        self.revision
    }
    /// The surviving resolving object whose instruction requires the choice.
    pub fn entry(&self) -> StackEntryId {
        self.entry
    }
    /// Suspended instruction index; selection does not advance this cursor.
    pub fn instruction(&self) -> usize {
        self.instruction
    }
    /// Historical chooser identity, which grants no current response authority.
    pub fn departed(&self) -> SeatId {
        self.departed
    }
    /// The object controller who selects the replacement.
    pub fn chooser(&self) -> SeatId {
        self.controller
    }
    /// Exact eligible current seats, in the maintained participant order.
    pub fn candidates(&self) -> &[SeatId] {
        &self.candidates
    }
}
impl OptionalEffectWork {
    pub(super) fn replacement_frame(&self, revision: u64) -> Option<ReplacementChooserFrame> {
        let substitution = self.substitution.as_ref()?;
        Some(ReplacementChooserFrame {
            revision,
            entry: self.cursor.entry,
            instruction: self.cursor.instruction,
            departed: self.chooser,
            controller: substitution.controller,
            candidates: substitution.candidates.clone(),
        })
    }
}
impl EffectWindow {
    pub(super) fn chooser_substitution(
        &self,
        departed: SeatId,
    ) -> Result<Option<ChooserSubstitution>, EffectError> {
        if self.window.participants().contains(&departed) {
            return Ok(None);
        }
        if !self.window.known_player(departed) {
            return Err(EffectError::InvalidResolvingPlayer);
        }
        let controller = self
            .window
            .resolving()
            .ok_or(EffectError::WrongResolutionFrame)?
            .controller();
        if !self.window.participants().contains(&controller) {
            return Err(EffectError::InconsistentProgram);
        }
        let mut candidates = self
            .window
            .participants()
            .iter()
            .copied()
            .filter(|seat| departed == controller || *seat != controller)
            .collect::<Vec<_>>();
        if candidates.is_empty() {
            candidates.extend(self.window.participants());
        }
        if candidates.is_empty() {
            return Err(EffectError::InconsistentProgram);
        }
        Ok(Some(ChooserSubstitution {
            controller,
            candidates,
        }))
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
            return Err(EffectError::WrongReplacementChooserFrame);
        }
        if actor != frame.controller {
            return Err(EffectError::WrongChooser);
        }
        let DecisionReply::ReplacementChooser(selected) = reply else {
            return Err(EffectError::WrongDecisionKind);
        };
        if !frame.candidates.contains(&selected) || !self.window.participants().contains(&selected)
        {
            return Err(EffectError::SelectionOutsideDomain);
        }
        let entry = self
            .window
            .resolving()
            .ok_or(EffectError::WrongResolutionFrame)?;
        if entry.id() != frame.entry || entry.controller() != frame.controller {
            return Err(EffectError::InconsistentProgram);
        }
        let mut next = self.clone();
        let Some(PendingWork::OptionalEffect(work)) = next.pending_work.as_mut() else {
            return Err(EffectError::WrongReplacementChooserFrame);
        };
        work.chooser = selected;
        work.substitution = None;
        next.revision = revision;
        let mut append = RulesJournal::default();
        append
            .records
            .push(crate::occurrence::RulesRecord::ChooserReplaced {
                entry: frame.entry,
                instruction: frame.instruction,
                departed: frame.departed,
                controller: actor,
                candidates: frame.candidates.clone(),
                selected,
            });
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
