//! CR 702.116. Choose independently for each other opponent, then create one simultaneous batch.
use super::*;
use crate::stack_window::StackEntry;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct MyriadChoices {
    opponents: Vec<SeatId>,
    next: usize,
    per_opponent: usize,
    selected_in_group: usize,
    groups: usize,
    defenders: Vec<(SeatId, TargetRef)>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MyriadDefenderFrame {
    revision: u64,
    entry: StackEntryId,
    instruction: usize,
    chooser: SeatId,
    opponent: SeatId,
    may_decline: bool,
    defenders: Vec<TargetRef>,
}
impl MyriadDefenderFrame {
    pub fn chooser(&self) -> SeatId {
        self.chooser
    }
    pub fn opponent(&self) -> SeatId {
        self.opponent
    }
    pub fn may_decline(&self) -> bool {
        self.may_decline
    }
    pub fn defenders(&self) -> impl Iterator<Item = TargetRef> + '_ {
        self.defenders.iter().copied()
    }
}

impl EffectWindow {
    fn myriad_choices_mut(
        &mut self,
        id: StackEntryId,
    ) -> Result<&mut Option<MyriadChoices>, EffectError> {
        let Some(StackPayload::Instructions(p)) = self.payloads.get_mut(&id) else {
            return Err(EffectError::InconsistentProgram);
        };
        Ok(&mut p.myriad)
    }

    fn myriad_defender_frame(
        &self,
        entry: &StackEntry,
    ) -> Result<Option<MyriadDefenderFrame>, EffectError> {
        let Some(StackPayload::Instructions(p)) = self.payloads.get(&entry.id()) else {
            return Err(EffectError::InconsistentProgram);
        };
        let choices = p.myriad.as_ref().ok_or(EffectError::InconsistentProgram)?;
        let Some(&opponent) = choices.opponents.get(choices.next) else {
            return Ok(None);
        };
        let current = self.evaluated_characteristics()?;
        let defenders = std::iter::once(TargetRef::Player(opponent))
            .chain(
                current
                    .objects
                    .values()
                    .filter(|o| {
                        o.zone == ObjectZone::Battlefield
                            && o.has_type(CardType::Planeswalker)
                            && o.controller == Some(opponent)
                    })
                    .map(|o| TargetRef::Object(o.object)),
            )
            .collect();
        Ok(Some(MyriadDefenderFrame {
            revision: self.revision,
            entry: entry.id(),
            instruction: self
                .instruction_index()
                .ok_or(EffectError::InconsistentProgram)?,
            chooser: entry.controller(),
            opponent,
            may_decline: choices.selected_in_group == 0,
            defenders,
        }))
    }

    pub(super) fn take_myriad_destinations(
        &mut self,
        entry: StackEntryId,
    ) -> Option<Vec<(SeatId, TargetRef)>> {
        let StackPayload::Instructions(p) = self.payloads.get_mut(&entry)? else {
            return None;
        };
        p.myriad.take().map(|choices| choices.defenders)
    }

    pub(super) fn progress_myriad(
        &mut self,
        entry: &StackEntry,
    ) -> Result<Option<MyriadDefenderFrame>, EffectError> {
        let trigger = self
            .execution_bindings(entry.id())
            .and_then(|b| b.trigger.as_ref())
            .ok_or(EffectError::InconsistentProgram)?;
        let source = trigger.source.object.id();
        let declared = trigger
            .causes
            .members()
            .first()
            .and_then(|member| member.related_player)
            .ok_or(EffectError::InconsistentProgram)?;
        if self.myriad_choices_mut(entry.id())?.is_none() {
            let defending = self
                .combat
                .as_ref()
                .and_then(|combat| combat.attacker(source))
                .map(|a| a.defender())
                .or_else(|| {
                    self.departed_attack_defenders
                        .as_ref()
                        .and_then(|values| values.get(&source).map(|(seat, _)| *seat))
                })
                .unwrap_or(declared);
            let opponents = self
                .combat_rules
                .opponents(self.window.participants(), entry.controller())?
                .into_iter()
                .filter(|&p| p != defending)
                .collect();
            let doublers =
                self.evaluated_characteristics()?
                    .controlled_rules(entry.controller(), |rule| {
                        matches!(
                            rule,
                            crate::characteristics::StaticContinuousAbility::Modifier {
                                rule:
                                    crate::characteristics::StaticModifierRule::DoubleCreatedTokens,
                                ..
                            }
                        )
                    });
            let per_opponent = 1usize
                .checked_shl(u32::try_from(doublers).map_err(|_| EffectError::TokenCapacity)?)
                .ok_or(EffectError::TokenCapacity)?;
            *self.myriad_choices_mut(entry.id())? = Some(MyriadChoices {
                opponents,
                next: 0,
                per_opponent,
                groups: 0,
                selected_in_group: 0,
                defenders: Vec::new(),
            });
        }
        if let Some(frame) = self.myriad_defender_frame(entry)? {
            return Ok(Some(frame));
        }
        let groups = self
            .myriad_choices_mut(entry.id())?
            .as_ref()
            .ok_or(EffectError::InconsistentProgram)?
            .groups;
        if groups == 0 {
            self.take_myriad_destinations(entry.id());
            self.advance_instruction()?;
            return Ok(None);
        }
        let definition = self
            .copiable_program(source)
            .or_else(|| self.departed_copy_sources.get(&source))
            .ok_or(EffectError::InconsistentProgram)?
            .clone();
        self.begin_materialized_token_entries(
            vec![(entry.controller(), groups, definition, true)],
            None,
            false,
        )?;
        Ok(None)
    }

    pub(super) fn resume_myriad(
        &self,
        expected: u64,
        frame: &MyriadDefenderFrame,
        actor: SeatId,
        defender: Option<TargetRef>,
    ) -> Result<RuleProgress, EffectError> {
        let revision = self.next_revision(expected)?;
        if actor != frame.chooser {
            return Err(EffectError::WrongChooser);
        }
        let entry = self
            .window
            .resolving()
            .ok_or(EffectError::WrongResolutionFrame)?;
        if self.current_instruction()? != &Instruction::Myriad
            || self.myriad_defender_frame(entry)?.as_ref() != Some(frame)
        {
            return Err(EffectError::WrongResolutionFrame);
        }
        if defender.is_none() && !frame.may_decline
            || defender.is_some_and(|target| !frame.defenders.contains(&target))
        {
            return Err(CombatError::IllegalDefender.into());
        }
        let mut next = self.clone();
        next.revision = revision;
        let choices = next
            .myriad_choices_mut(entry.id())?
            .as_mut()
            .ok_or(EffectError::InconsistentProgram)?;
        if let Some(target) = defender {
            if choices.selected_in_group == 0 {
                choices.groups += 1;
            }
            choices.defenders.push((frame.opponent, target));
            choices.selected_in_group += 1;
            if frame.defenders.len() == 1 {
                choices
                    .defenders
                    .try_reserve(choices.per_opponent - choices.selected_in_group)
                    .map_err(|_| EffectError::TokenCapacity)?;
                choices.defenders.extend(std::iter::repeat_n(
                    (frame.opponent, target),
                    choices.per_opponent - choices.selected_in_group,
                ));
                choices.selected_in_group = choices.per_opponent;
            }
        }
        if defender.is_none() || choices.selected_in_group == choices.per_opponent {
            choices.selected_in_group = 0;
            choices.next += 1;
        }
        next.progress_rules(
            DamageJournal::default(),
            vec![],
            vec![],
            DrawJournal::default(),
            RulesJournal::default(),
        )
    }
}
