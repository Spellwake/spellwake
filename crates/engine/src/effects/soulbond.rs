//! The two triggered abilities of soulbond and their shared non-targeting choice (CR 702.95).
use super::*;
use crate::{characteristics::EvaluatedCharacteristics, occurrence::TriggerRelated};

impl EffectWindow {
    pub(super) fn soulbond_selection(
        &self,
        entrant_only: bool,
    ) -> Result<Option<ResolutionSelectionFrame>, EffectError> {
        let entry = self
            .window
            .resolving()
            .ok_or(EffectError::WrongResolutionFrame)?;
        let cursor = self
            .resolution_cursor()
            .ok_or(EffectError::WrongResolutionFrame)?;
        let binding = self
            .execution_bindings(entry.id())
            .and_then(|bindings| bindings.trigger.as_ref())
            .ok_or(EffectError::InconsistentProgram)?;
        let source = binding.source.object.id();
        let player = entry.controller();
        let current = self.evaluated_characteristics()?;
        let unpaired = |id| {
            current
                .get(id)
                .is_some_and(|view| is_controlled_creature(view, player))
                && !self.characteristics.paired_creatures.contains_key(&id)
        };
        if !unpaired(source) {
            return Ok(None);
        }
        let domain = if entrant_only {
            binding
                .causes
                .members()
                .iter()
                .filter_map(|cause| match &cause.related {
                    TriggerRelated::ZoneChange { after, .. }
                    | TriggerRelated::TokenCreated { after, .. } => {
                        let entrant = after.object.id();
                        (entrant != source && unpaired(entrant)).then_some(entrant)
                    }
                    _ => None,
                })
                .collect::<Vec<_>>()
        } else {
            current
                .objects
                .keys()
                .copied()
                .filter(|&id| id != source && unpaired(id))
                .collect()
        };
        Ok((!domain.is_empty()).then(|| {
            ResolutionSelectionFrame::hand_entry(
                self.revision,
                entry.id(),
                cursor.instruction,
                player,
                domain,
                1,
                ResolutionSelectionKind::Soulbond,
            )
        }))
    }

    pub(super) fn resume_soulbond(
        &self,
        expected: u64,
        frame: &ResolutionSelectionFrame,
        actor: SeatId,
        reply: DecisionReply,
    ) -> Result<RuleProgress, EffectError> {
        let revision = self.next_revision(expected)?;
        let entrant_only = match self.current_instruction()? {
            Instruction::MutateObject(spec) => match &spec.op {
                crate::toolbox::ObjectMutation::SoulbondPair { entrant_only } => entrant_only,
                _ => return Err(EffectError::WrongResolutionFrame),
            },
            _ => return Err(EffectError::WrongResolutionFrame),
        };
        if self.soulbond_selection(*entrant_only)?.as_ref() != Some(frame) {
            return Err(EffectError::WrongResolutionFrame);
        }
        if actor != frame.chooser() {
            return Err(EffectError::WrongChooser);
        }
        let DecisionReply::SelectResolution(selected) = reply else {
            return Err(EffectError::WrongDecisionKind);
        };
        if selected.len() > 1
            || selected
                .first()
                .is_some_and(|id| !frame.domain().contains(id))
        {
            return Err(EffectError::IllegalTarget);
        }
        let mut next = self.clone();
        if let Some(&partner) = selected.first() {
            let source = self
                .execution_bindings(frame.entry())
                .and_then(|bindings| bindings.trigger.as_ref())
                .ok_or(EffectError::InconsistentProgram)?
                .source
                .object
                .id();
            next.characteristics
                .paired_creatures
                .insert(source, partner);
            next.characteristics
                .paired_creatures
                .insert(partner, source);
        }
        next.advance_instruction()?;
        next.revision = revision;
        next.progress_rules(
            DamageJournal::default(),
            vec![],
            vec![],
            DrawJournal::default(),
            RulesJournal::default(),
        )
    }

    pub(super) fn reconcile_pairs(
        &mut self,
        evaluated: &EvaluatedCharacteristics,
        changes: &[crate::object_identity::ControllerChange],
    ) -> bool {
        let pairs = &mut self.characteristics.paired_creatures;
        let previous = pairs.len();
        pairs.retain(|&first, second| {
            let second = *second;
            if changes
                .iter()
                .any(|change| change.after.id() == first || change.after.id() == second)
            {
                return false;
            }
            // The pairing's “for as long as” duration cannot track a phased-out partner.
            evaluated
                .get(first)
                .zip(evaluated.get(second))
                .is_some_and(|(left, right)| {
                    left.controller.is_some_and(|controller| {
                        is_controlled_creature(left, controller)
                            && is_controlled_creature(right, controller)
                    })
                })
        });
        pairs.len() != previous
    }
}

fn is_controlled_creature(view: &ObjectCharacteristics, player: SeatId) -> bool {
    view.zone == ObjectZone::Battlefield && view.is_creature() && view.controller == Some(player)
}
