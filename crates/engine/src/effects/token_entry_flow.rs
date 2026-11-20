//! One simultaneous creation cut, with relative order chosen before actual birth.
use super::*;
use crate::token::{TokenCreation, TokenEntryPreview, TokenRecipients};

#[derive(Debug, Clone, PartialEq, Eq)]
struct TimestampGroup {
    chooser: SeatId,
    members: Vec<TokenBatchMember>,
}
/// The existing cursor moves here; previews are not another current-object store.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct TokenEntryWork {
    pub(super) cursor: ResolutionCursor,
    members: Vec<TokenEntryPreview>,
    groups: Vec<TimestampGroup>,
    next_group: usize,
    timestamp_order: Vec<TokenBatchMember>,
}

impl EffectWindow {
    /// Keep original preview coordinates and accepted survivor order. These are
    /// pre-birth values, never objects to remove or player replies to fabricate.
    pub(super) fn refresh_token_entries_after_departure(&mut self) -> Result<(), EffectError> {
        let live = self
            .window
            .participants()
            .iter()
            .copied()
            .collect::<BTreeSet<_>>();
        let Some(PendingWork::TokenEntry(work)) = &mut self.pending_work else {
            return Err(EffectError::WrongPhase);
        };
        if self.window.resolving().map(|entry| entry.id()) != Some(work.cursor.entry) {
            if self
                .window
                .entries()
                .iter()
                .any(|entry| entry.id() == work.cursor.entry)
            {
                return Err(EffectError::InconsistentProgram);
            }
            self.pending_work = None;
            return Ok(());
        }
        if work.next_group > work.groups.len()
            || work.members.iter().enumerate().any(|(index, member)| {
                member.member.ordinal() != index || !self.window.known_player(member.creator)
            })
        {
            return Err(EffectError::InconsistentProgram);
        }
        let mut grouped = BTreeSet::new();
        for group in &work.groups {
            if !self.window.known_player(group.chooser) {
                return Err(EffectError::InconsistentProgram);
            }
            for member in &group.members {
                let preview = work
                    .members
                    .get(member.ordinal())
                    .ok_or(EffectError::InconsistentProgram)?;
                if preview.member != *member
                    || preview.creator != group.chooser
                    || !grouped.insert(*member)
                {
                    return Err(EffectError::InconsistentProgram);
                }
            }
        }
        let retained = work
            .members
            .iter()
            .filter(|member| live.contains(&member.creator))
            .map(|member| member.member)
            .collect::<BTreeSet<_>>();
        let prefix = work
            .groups
            .iter()
            .take(work.next_group)
            .flat_map(|group| group.members.iter().copied())
            .collect::<BTreeSet<_>>();
        let accepted = work
            .timestamp_order
            .iter()
            .copied()
            .collect::<BTreeSet<_>>();
        if !retained.is_subset(&grouped)
            || accepted.len() != work.timestamp_order.len()
            || accepted != prefix
        {
            return Err(EffectError::InconsistentProgram);
        }
        work.timestamp_order
            .retain(|member| retained.contains(member));
        for group in &mut work.groups {
            group.members.retain(|member| retained.contains(member));
        }
        Ok(())
    }

    pub(super) fn begin_token_entries(
        &mut self,
        creations: &[TokenCreation],
    ) -> Result<(), EffectError> {
        let entry = self.window.resolving().ok_or(EffectError::WrongPhase)?;
        let controller = entry.controller();
        let mut groups = Vec::new();
        let mut total = 0usize;
        // All counts read the same actual pre-batch state and resolution locals.
        for creation in creations {
            let amount = self.resolve_instruction_amount(controller, &creation.amount)?;
            let count = String::from(amount)
                .parse::<usize>()
                .map_err(|_| EffectError::TokenCapacity)?;
            let definition = creation
                .definition
                .materialize(&self.creature_vocabulary)
                .map_err(|_| EffectError::InconsistentProgram)?;
            for creator in
                self.window.participants().iter().copied().filter(|seat| {
                    match creation.recipients {
                        TokenRecipients::Controller => *seat == controller,
                        TokenRecipients::EachPlayer => true,
                        TokenRecipients::EachOpponent => *seat != controller,
                    }
                })
            {
                total = total.checked_add(count).ok_or(EffectError::TokenCapacity)?;
                groups.push((creator, count, definition.clone()));
            }
        }
        let count = u64::try_from(total).map_err(|_| EffectError::TokenCapacity)?;
        self.window
            .cards()
            .revision()
            .checked_add(count)
            .ok_or(StackWindowError::Objects(
                crate::object_identity::ObjectIdentityError::RevisionExhausted,
            ))?;
        self.window
            .cards()
            .next_identity_sequence()
            .checked_add(count)
            .ok_or(StackWindowError::Objects(
                crate::object_identity::ObjectIdentityError::IdentityExhausted,
            ))?;
        let mut members = Vec::new();
        members
            .try_reserve_exact(total)
            .map_err(|_| EffectError::TokenCapacity)?;
        for (creator, count, definition) in groups {
            for _ in 0..count {
                members.push(TokenEntryPreview {
                    member: TokenBatchMember::new(members.len()),
                    creator,
                    definition: definition.clone(),
                });
            }
        }
        if members.is_empty() {
            let cursor = self
                .resolution_cursor()
                .ok_or(EffectError::WrongResolutionFrame)?;
            let (entry, instruction) = (cursor.entry, cursor.instruction);
            self.advance_instruction()?;
            #[cfg(test)]
            characteristic_observations::instruction(self, "Completed", entry, instruction, None);
            #[cfg(not(test))]
            let _ = (entry, instruction);
            return Ok(());
        }
        let mut groups = Vec::new();
        for chooser in self.window.apnap_players()? {
            let offered = members
                .iter()
                .filter(|m| m.creator == chooser)
                .map(|m| m.member)
                .collect::<Vec<_>>();
            if !offered.is_empty() {
                groups.push(TimestampGroup {
                    chooser,
                    members: offered,
                });
            }
        }
        let Some(PendingWork::Resolution(cursor)) = self.pending_work.take() else {
            return Err(EffectError::WrongResolutionFrame);
        };
        self.pending_work = Some(PendingWork::TokenEntry(Box::new(TokenEntryWork {
            cursor,
            members,
            groups,
            next_group: 0,
            timestamp_order: Vec::new(),
        })));
        Ok(())
    }

    pub(super) fn token_timestamp_frame(&self) -> Result<TokenTimestampOrder, EffectError> {
        let Some(PendingWork::TokenEntry(work)) = &self.pending_work else {
            return Err(EffectError::WrongTimestampOrderFrame);
        };
        let group = work
            .groups
            .get(work.next_group)
            .ok_or(EffectError::WrongTimestampOrderFrame)?;
        if group.members.len() < 2
            || self.window.resolving().map(|e| e.id()) != Some(work.cursor.entry)
        {
            return Err(EffectError::WrongTimestampOrderFrame);
        }
        let members = group
            .members
            .iter()
            .map(|member| {
                work.members
                    .get(member.ordinal())
                    .cloned()
                    .ok_or(EffectError::InconsistentProgram)
            })
            .collect::<Result<_, _>>()?;
        Ok(TokenTimestampOrder {
            revision: self.revision,
            entry: work.cursor.entry,
            instruction: work.cursor.instruction,
            chooser: group.chooser,
            members,
        })
    }

    pub(super) fn accept_token_timestamp_order(
        &self,
        expected: u64,
        frame: &TokenTimestampOrder,
        actor: SeatId,
        order: Vec<TokenBatchMember>,
    ) -> Result<Self, EffectError> {
        let revision = self.next_revision(expected)?;
        if &self.token_timestamp_frame()? != frame {
            return Err(EffectError::WrongTimestampOrderFrame);
        }
        if actor != frame.chooser() {
            return Err(EffectError::WrongChooser);
        }
        let supplied = order.iter().copied().collect::<BTreeSet<_>>();
        let offered = frame
            .members()
            .iter()
            .map(|m| m.member)
            .collect::<BTreeSet<_>>();
        if order.len() != offered.len() || supplied.len() != order.len() || supplied != offered {
            return Err(EffectError::IllegalTimestampOrder);
        }
        let mut next = self.clone();
        let Some(PendingWork::TokenEntry(work)) = &mut next.pending_work else {
            return Err(EffectError::WrongTimestampOrderFrame);
        };
        work.timestamp_order.extend(order);
        work.next_group = work
            .next_group
            .checked_add(1)
            .ok_or(EffectError::TokenCapacity)?;
        next.revision = revision;
        Ok(next)
    }

    /// Advance forced groups or publish one complete batch; no partial entrant is
    /// observable during ordering. The shared driver handles the returned frame.
    pub(super) fn progress_token_entries(
        &mut self,
        append: &mut RulesJournal,
    ) -> Result<Option<TokenTimestampOrder>, EffectError> {
        let Some(PendingWork::TokenEntry(work)) = &mut self.pending_work else {
            return Err(EffectError::WrongPhase);
        };
        while let Some(group) = work.groups.get(work.next_group) {
            if group.members.len() > 1 {
                return self.token_timestamp_frame().map(Some);
            }
            work.timestamp_order.extend(group.members.iter().copied());
            work.next_group = work
                .next_group
                .checked_add(1)
                .ok_or(EffectError::TokenCapacity)?;
        }
        let Some(PendingWork::TokenEntry(work)) = self.pending_work.take() else {
            return Err(EffectError::WrongPhase);
        };
        let TokenEntryWork {
            cursor,
            members,
            timestamp_order,
            ..
        } = *work;
        let retained = members
            .iter()
            .filter(|member| self.window.participants().contains(&member.creator))
            .collect::<Vec<_>>();
        let expected = retained
            .iter()
            .map(|member| member.member)
            .collect::<BTreeSet<_>>();
        if expected.len() != retained.len()
            || timestamp_order.len() != retained.len()
            || timestamp_order.iter().copied().collect::<BTreeSet<_>>() != expected
        {
            return Err(EffectError::InconsistentProgram);
        }
        if retained.is_empty() {
            let (entry, instruction) = (cursor.entry, cursor.instruction);
            self.pending_work = Some(PendingWork::Resolution(cursor));
            self.advance_instruction()?;
            #[cfg(test)]
            characteristic_observations::instruction(self, "Completed", entry, instruction, None);
            #[cfg(not(test))]
            let _ = (entry, instruction);
            return Ok(None);
        }
        let before = self.rules_snapshot(None)?;
        let inputs = retained
            .iter()
            .map(|m| (m.creator, m.definition.clone()))
            .collect();
        let (window, objects) = self.window.record_token_creations(
            self.window.revision(),
            inputs,
            &self.creature_vocabulary,
        )?;
        if objects.len() != retained.len() {
            return Err(EffectError::InconsistentProgram);
        }
        let births = retained
            .iter()
            .zip(&objects)
            .map(|(member, object)| (member.member, object.id()))
            .collect::<BTreeMap<_, _>>();
        let order = timestamp_order
            .iter()
            .map(|member| {
                births
                    .get(member)
                    .copied()
                    .ok_or(EffectError::InconsistentProgram)
            })
            .collect::<Result<Vec<_>, _>>()?;
        self.window = window;
        self.characteristics.register_battlefield_entries(&order)?;
        let entry = cursor.entry;
        let instruction = cursor.instruction;
        self.pending_work = Some(PendingWork::Resolution(cursor));
        self.emit_from(
            crate::occurrence::EventCause::Instruction { entry, instruction },
            &before,
            None,
            append,
            |_, current, _| {
                objects
                    .iter()
                    .map(|object| {
                        Ok(crate::occurrence::RulesOccurrence::TokenCreated {
                            creator: object.owner(),
                            after: Box::new(
                                current
                                    .appearance(object.id())
                                    .ok_or(EffectError::InconsistentProgram)?,
                            ),
                        })
                    })
                    .collect()
            },
        )?;
        self.advance_instruction()?;
        #[cfg(test)]
        characteristic_observations::instruction(self, "Completed", entry, instruction, None);
        Ok(None)
    }
}
