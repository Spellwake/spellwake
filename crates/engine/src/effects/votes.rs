//! Sequential public card ballots, ordered from the resolving controller around the table.
use super::*;
use crate::occurrence::{EventCause, RulesOccurrence};

impl EffectWindow {
    /// Earlier public votes in the current card ballot, in actual voting order.
    pub fn public_graveyard_votes(&self) -> &[(SeatId, ObjectId)] {
        self.resolution_cursor()
            .map_or(&[], |cursor| cursor.impulse.votes.as_slice())
    }

    pub(super) fn graveyard_vote_frame(
        &self,
        types: &BTreeSet<CardType>,
    ) -> Result<Option<ResolutionSelectionFrame>, EffectError> {
        let entry = self
            .window
            .resolving()
            .ok_or(EffectError::WrongResolutionFrame)?;
        let cursor = self
            .resolution_cursor()
            .ok_or(EffectError::WrongResolutionFrame)?;
        let seats = self.window.participants();
        let start = seats
            .iter()
            .position(|&seat| seat == entry.controller())
            .ok_or(EffectError::InconsistentProgram)?;
        let Some(&chooser) = seats
            .iter()
            .cycle()
            .skip(start)
            .take(seats.len())
            .nth(cursor.impulse.votes.len())
        else {
            return Ok(None);
        };
        let current = self.evaluated_characteristics()?;
        let domain = self
            .window
            .cards()
            .graveyard(entry.controller())
            .iter()
            .copied()
            .filter(|&id| {
                current
                    .get(id)
                    .is_some_and(|card| !card.current_types.is_disjoint(types))
            })
            .collect::<Vec<_>>();
        Ok((!domain.is_empty()).then(|| {
            ResolutionSelectionFrame::hand_entry(
                self.revision,
                entry.id(),
                cursor.instruction,
                chooser,
                domain,
                1,
                ResolutionSelectionKind::VoteForCard,
            )
        }))
    }

    pub(super) fn resume_graveyard_vote(
        &self,
        expected: u64,
        frame: &ResolutionSelectionFrame,
        actor: SeatId,
        reply: DecisionReply,
    ) -> Result<RuleProgress, EffectError> {
        let revision = self.next_revision(expected)?;
        let Instruction::VoteForGraveyardCards { types } = self.current_instruction()? else {
            return Err(EffectError::WrongResolutionFrame);
        };
        if self.graveyard_vote_frame(types)?.as_ref() != Some(frame) {
            return Err(EffectError::WrongResolutionFrame);
        }
        if actor != frame.chooser() {
            return Err(EffectError::WrongChooser);
        }
        let DecisionReply::SelectResolution(objects) = reply else {
            return Err(EffectError::WrongDecisionKind);
        };
        let [object] = objects.as_slice() else {
            return Err(EffectError::IllegalTarget);
        };
        if !frame.domain().contains(object) {
            return Err(EffectError::IllegalTarget);
        }
        let mut next = self.clone();
        let before = next.rules_snapshot(None)?;
        next.resolution_cursor_mut()
            .ok_or(EffectError::WrongResolutionFrame)?
            .impulse
            .votes
            .push((actor, *object));
        let mut append = RulesJournal::default();
        next.emit_from(
            EventCause::Instruction {
                entry: frame.entry(),
                instruction: frame.instruction(),
            },
            &before,
            None,
            &mut append,
            |_, _, _| {
                Ok(vec![RulesOccurrence::CardVoteCast {
                    player: actor,
                    object: *object,
                }])
            },
        )?;
        next.revision = revision;
        next.progress_rules(
            DamageJournal::default(),
            vec![],
            vec![],
            DrawJournal::default(),
            append,
        )
    }

    pub(super) fn return_vote_winners(&mut self) -> Result<bool, EffectError> {
        let cursor = self
            .resolution_cursor_mut()
            .ok_or(EffectError::WrongResolutionFrame)?;
        let mut counts = BTreeMap::new();
        for (_, object) in std::mem::take(&mut cursor.impulse.votes) {
            *counts.entry(object).or_insert(0usize) += 1;
        }
        let Some(maximum) = counts.values().max().copied() else {
            return Ok(false);
        };
        let winners = counts
            .into_iter()
            .filter_map(|(object, total)| (total == maximum).then_some(object))
            .collect();
        self.begin_public_cards_to_hand(winners)
    }
}
