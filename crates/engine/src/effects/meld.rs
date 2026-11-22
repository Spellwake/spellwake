//! Meld checks the names in play and separately checks both printed cards in exile (CR 713).
use super::*;
use crate::object_identity::ObjectMovement;
use crate::occurrence::{EventCause, ZoneReason};
use spellwake_contracts::names::EnglishCardName;

impl EffectWindow {
    fn meld_source(&self, entry: StackEntryId) -> Result<ObjectId, EffectError> {
        self.execution_bindings(entry)
            .and_then(|b| b.trigger.as_ref())
            .map(|b| b.source.object.id())
            .ok_or(EffectError::WrongResolutionFrame)
    }

    pub(super) fn meld_selection(
        &self,
        mate: &EnglishCardName,
    ) -> Result<Option<ResolutionSelectionFrame>, EffectError> {
        let entry = self
            .window
            .resolving()
            .ok_or(EffectError::WrongResolutionFrame)?;
        let cursor = self
            .resolution_cursor()
            .ok_or(EffectError::WrongResolutionFrame)?;
        let source = self.meld_source(entry.id())?;
        let player = entry.controller();
        let current = self.evaluated_characteristics()?;
        let owned = |v: &ObjectCharacteristics| {
            v.zone == ObjectZone::Battlefield && v.owner == player && v.controller == Some(player)
        };
        if !current.get(source).is_some_and(owned) {
            return Ok(None);
        }
        let domain: Vec<_> = current
            .objects
            .values()
            .filter(|v| {
                v.object != source
                    && owned(v)
                    && v.is_creature()
                    && self.name_rules.has_name(&v.current_names, mate)
            })
            .map(|v| v.object)
            .collect();
        Ok((!domain.is_empty()).then(|| {
            ResolutionSelectionFrame::hand_entry(
                self.revision,
                entry.id(),
                cursor.instruction,
                player,
                domain,
                1,
                ResolutionSelectionKind::MeldPartner,
            )
        }))
    }

    pub(super) fn resume_meld(
        &self,
        expected: u64,
        frame: &ResolutionSelectionFrame,
        actor: SeatId,
        reply: DecisionReply,
    ) -> Result<RuleProgress, EffectError> {
        let revision = self.next_revision(expected)?;
        let Instruction::ChooseMeldPartner(mate) = self.current_instruction()? else {
            return Err(EffectError::WrongResolutionFrame);
        };
        if self.meld_selection(mate)?.as_ref() != Some(frame) {
            return Err(EffectError::WrongResolutionFrame);
        }
        if actor != frame.chooser() {
            return Err(EffectError::WrongChooser);
        }
        let DecisionReply::SelectResolution(chosen) = reply else {
            return Err(EffectError::WrongDecisionKind);
        };
        let [other] = chosen.as_slice() else {
            return Err(EffectError::IllegalTarget);
        };
        if !frame.domain().contains(other) {
            return Err(EffectError::IllegalTarget);
        }
        let pair = [self.meld_source(frame.entry())?, *other].map(|id| {
            (
                id,
                self.window.cards().get(id).and_then(GameObject::card_id),
            )
        });
        let mut next = self.clone();
        next.resolution_cursor_mut()
            .ok_or(EffectError::WrongResolutionFrame)?
            .meld_pair = Some(pair);
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

    pub(super) fn finish_meld(
        &mut self,
        back: &Program,
        journal: &mut RulesJournal,
    ) -> Result<(), EffectError> {
        let cursor = self
            .resolution_cursor()
            .ok_or(EffectError::WrongResolutionFrame)?;
        let Some([(_, Some(first)), (_, Some(second))]) = cursor.meld_pair else {
            return Ok(());
        };
        let Some((left, right)) = self.programs.get(&first).zip(self.programs.get(&second)) else {
            return Ok(());
        };
        let genuine = left
            .meld_partner
            .as_ref()
            .is_some_and(|p| right.names.iter().any(|n| n == p))
            && right
                .meld_partner
                .as_ref()
                .is_some_and(|p| left.names.iter().any(|n| n == p));
        let same_result=left.triggered.iter().chain(&right.triggered).flat_map(|t|&t.body.instructions)
            .any(|op|matches!(op, Instruction::MeldExiledPair(printed) if printed.names==back.names));
        if !genuine || !same_result {
            return Ok(());
        }
        let in_exile = |card| {
            self.window
                .cards()
                .current_identity(card)
                .and_then(|id| self.window.cards().get(id))
                .filter(|c| c.zone() == ObjectZone::Exile)
        };
        let Some((first, second)) = in_exile(first).zip(in_exile(second)) else {
            return Ok(());
        };
        let (first, second) = (first.clone(), second.id());
        let (entry, instruction) = (cursor.entry, cursor.instruction);
        let controller = self
            .window
            .resolving()
            .ok_or(EffectError::WrongResolutionFrame)?
            .controller();
        let before = self.rules_snapshot(None)?;
        let joined = self.window.meld_during_resolution(
            entry,
            [first.id(), second],
            controller,
            back.clone(),
        )?;
        let id = joined.id();
        self.characteristics.register_battlefield_entry(id)?;
        let counters =
            self.apply_entry_counters(&before, id, PowerToughnessCounters::default(), 0)?;
        let joined = self
            .window
            .cards()
            .get(id)
            .ok_or(EffectError::InconsistentProgram)?
            .clone();
        let old_id = first.id();
        self.emit_from(
            EventCause::Instruction { entry, instruction },
            &before,
            None,
            journal,
            |old, now, controllers| {
                let mut rows = vec![occurrence_flow::zone_occurrence_at(
                    old,
                    now,
                    &ObjectMovement {
                        before: first,
                        after: joined,
                    },
                    ZoneReason::ExileToBattlefield,
                    controllers,
                )?];
                if counters.valid_placement() {
                    rows.push(crate::occurrence::RulesOccurrence::CountersPlaced {
                        object: id,
                        before: old.characteristics.counters(id),
                        after: now.characteristics.counters(id),
                        added: counters,
                        before_appearance: Box::new(
                            old.appearance(old_id)
                                .ok_or(EffectError::InconsistentProgram)?,
                        ),
                        after_appearance: Box::new(
                            now.appearance(id).ok_or(EffectError::InconsistentProgram)?,
                        ),
                    });
                }
                Ok(rows)
            },
        )
    }
}
