//! Top-library reveal and explore choices.
use super::*;
use crate::{
    occurrence::{EventCause, ZoneReason},
    program::ZoneActionKind,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct LookTopWork {
    pub(super) cursor: ResolutionCursor,
    player: SeatId,
    exploring: Option<GameObject>,
    explore_nonland: bool,
    remaining_explores: u8,
    domain: Vec<ObjectId>,
    selected: Vec<ObjectId>,
    decided: bool,
    pub(super) replacement: Option<ZoneReplacementBatch>,
}
impl LookTopWork {
    fn choice(&self, revision: u64) -> Option<EffectDecision> {
        if self.decided {
            return self
                .replacement
                .as_ref()
                .and_then(|batch| batch.frame(revision))
                .map(EffectDecision::ZoneReplacement);
        }
        Some(EffectDecision::LibraryTop(
            LibraryTopFrame::looked_top(
                revision,
                self.cursor.entry,
                self.cursor.instruction,
                self.player,
                self.domain.clone(),
                self.domain.clone(),
                false,
                true,
                false,
                false,
                None,
                self.explore_nonland,
            )
            .with_reveal_only(self.exploring.is_none()),
        ))
    }
}
impl EffectWindow {
    pub(super) fn currently_revealed_look_top(&self, id: ObjectId) -> bool {
        matches!(
            &self.pending_work,
            Some(PendingWork::LookTop(work))
                if work.exploring.is_some() && work.domain.first() == Some(&id)
        )
    }
    fn extra_explore_count(&self, source: &GameObject) -> Result<u8, EffectError> {
        let evaluated = self.evaluated_characteristics()?;
        let Some(src_view) = evaluated.get(source.id()) else {
            return Ok(0);
        };
        let mut total: u16 = 1;
        let excluded = BTreeSet::new();
        let mut check_rule =
            |ctrl: SeatId, ab: &crate::characteristics::StaticContinuousAbility| {
                let crate::characteristics::StaticContinuousAbility::EventReplacement {
                    event: crate::characteristics::ReplaceableEvent::Explore { filter },
                    action:
                        crate::characteristics::StaticReplacementAction::ModifyAmount { add, multiply },
                    ..
                } = ab
                else {
                    return;
                };
                let matches_ctrl = match filter {
                    crate::program::ObjectFilter::Clause(c) => match c.controller {
                        crate::program::ControllerFilter::Any => true,
                        crate::program::ControllerFilter::You => src_view.controller == Some(ctrl),
                        crate::program::ControllerFilter::Opponent => {
                            src_view.controller.is_some_and(|s| s != ctrl)
                        }
                        _ => true,
                    },
                    crate::program::ObjectFilter::Either(_, _) => true,
                };
                if matches_ctrl
                    && filter.matches_static_characteristics(src_view, &excluded, || {
                        self.permanent_mana_value(source.id())
                    })
                {
                    if let crate::program::ValueExpr::Literal(v) = add.as_ref()
                        && let Some(u) = v.to_usize()
                    {
                        total = total.saturating_add(u16::try_from(u).unwrap_or(8));
                    }
                    if *multiply > 1 {
                        total = total.saturating_mul(u16::from(*multiply));
                    }
                    total = total.min(8);
                }
            };
        for perm in evaluated
            .objects
            .values()
            .filter(|p| p.zone == ObjectZone::Battlefield)
        {
            let Some(ctrl) = perm.controller else {
                continue;
            };
            for ab in perm
                .printed_text
                .continuous
                .iter()
                .chain(&perm.printed_text.additional_battlefield_abilities)
            {
                let active = perm.abilities.continuous == Some(ab.id())
                    || perm
                        .abilities
                        .additional_battlefield_abilities
                        .contains(&ab.id());
                if active {
                    check_rule(ctrl, ab);
                }
            }
        }
        for active in &self.temporary_rules {
            if let Some(ab) = self.active_temporary_static(active, &evaluated) {
                check_rule(active.controller, ab);
            }
        }
        Ok(u8::try_from(total.saturating_sub(1)).unwrap_or(7))
    }

    fn explore_counter(
        &mut self,
        source: &GameObject,
        cursor: &ResolutionCursor,
        journal: &mut RulesJournal,
    ) -> Result<(), EffectError> {
        if self
            .window
            .cards()
            .get(source.id())
            .is_some_and(|object| object.zone() == ObjectZone::Battlefield)
        {
            self.add_source_counters(
                source.id(),
                cursor.entry,
                cursor.instruction,
                1.into(),
                resolution_choices::CounterPlacement::Ordinary,
                journal,
            )?;
        }
        Ok(())
    }
    fn explored(
        &mut self,
        source: GameObject,
        controller: SeatId,
        cursor: &ResolutionCursor,
        journal: &mut RulesJournal,
    ) -> Result<(), EffectError> {
        let before = self.rules_snapshot(None)?;
        self.emit_from(
            EventCause::Instruction {
                entry: cursor.entry,
                instruction: cursor.instruction,
            },
            &before,
            None,
            journal,
            |_, _, _| {
                Ok(vec![crate::occurrence::RulesOccurrence::Explored {
                    source,
                    controller,
                }])
            },
        )
    }
    pub(super) fn look_top_frame(&self) -> Option<EffectDecision> {
        match &self.pending_work {
            Some(PendingWork::LookTop(work)) => work.choice(self.revision),
            _ => None,
        }
    }
    fn start_explore_step(
        &mut self,
        cursor: ResolutionCursor,
        player: SeatId,
        source: GameObject,
        card_id: ObjectId,
        remaining_explores: u8,
        append: &mut RulesJournal,
    ) -> Result<LookTopWork, EffectError> {
        let evaluated = self.evaluated_characteristics()?;
        let explore_nonland = evaluated
            .get(card_id)
            .is_some_and(|card| !card.has_type(CardType::Land));
        let object = self
            .window
            .cards()
            .get(card_id)
            .ok_or(EffectError::UnknownObject)?
            .clone();
        let before = self.rules_snapshot(None)?;
        self.emit_from(
            EventCause::Instruction {
                entry: cursor.entry,
                instruction: cursor.instruction,
            },
            &before,
            None,
            append,
            |_, _, _| {
                Ok(vec![
                    crate::occurrence::RulesOccurrence::LibraryCardRevealed { object },
                ])
            },
        )?;
        if explore_nonland {
            self.explore_counter(&source, &cursor, append)?;
        }
        let mut work = LookTopWork {
            cursor,
            player,
            exploring: Some(source),
            explore_nonland,
            remaining_explores,
            domain: vec![card_id],
            selected: Vec::new(),
            decided: false,
            replacement: None,
        };
        if !explore_nonland {
            work.selected.push(card_id);
            work.decided = true;
            work.replacement = Some(self.look_top_batch(&work)?);
        }
        Ok(work)
    }
    pub(super) fn begin_look_top(
        &mut self,
        instruction: &Instruction,
        append: &mut RulesJournal,
    ) -> Result<bool, EffectError> {
        let is_may_reveal_library_top = matches!(
            instruction,
            Instruction::MutatePlayer(spec)
                if matches!(
                    spec.op,
                    crate::toolbox::PlayerMutation::LibraryTop(
                        crate::toolbox::LibraryTopAction::MayRevealTop
                    )
                )
        );
        let is_explore = matches!(
            instruction,
            Instruction::MutateObject(spec)
                if matches!(spec.op, crate::toolbox::ObjectMutation::Explore)
        );
        if !is_may_reveal_library_top && !is_explore {
            return Ok(false);
        }
        self.resolution_cursor()
            .ok_or(EffectError::WrongResolutionFrame)?;
        if is_may_reveal_library_top {
            let results = &mut self
                .resolution_cursor_mut()
                .ok_or(EffectError::WrongResolutionFrame)?
                .results;
            results.revealed_top_types.clear();
            results.revealed_top_card = None;
        }
        let entry = self.window.resolving().ok_or(EffectError::WrongPhase)?;
        let exploring = if let Instruction::MutateObject(spec) = instruction
            && matches!(spec.op, crate::toolbox::ObjectMutation::Explore)
        {
            let resolved_id = self
                .eval_object_query_at(
                    entry.controller(),
                    Some(entry),
                    &spec.source,
                    self.resolution_cursor(),
                )?
                .into_iter()
                .next();
            if let Some(card) = resolved_id
                .and_then(|id| self.window.cards().get(id))
                .filter(|c| c.zone() == ObjectZone::Battlefield)
            {
                Some(card.clone())
            } else {
                self.advance_instruction()?;
                return Ok(true);
            }
        } else {
            None
        };
        let player = exploring
            .as_ref()
            .and_then(|source| {
                self.window
                    .cards()
                    .get(source.id())
                    .and_then(GameObject::controller)
            })
            .unwrap_or(entry.controller());
        let top_card = self.window.cards().library(player).first().copied();
        let extra_explores = match &exploring {
            Some(source) => self.extra_explore_count(source)?,
            None => 0,
        };
        let Some(card_id) = top_card else {
            if let Some(source) = &exploring {
                let cursor = self
                    .resolution_cursor()
                    .ok_or(EffectError::WrongResolutionFrame)?
                    .clone();
                for _ in 0..=extra_explores {
                    self.explore_counter(source, &cursor, append)?;
                    self.explored(source.clone(), player, &cursor, append)?;
                }
            } else if let Some(cursor) = self.resolution_cursor_mut() {
                cursor.results.last_action_succeeded = false;
                cursor.results.last_action_count = Some(0.into());
            }
            self.advance_instruction()?;
            return Ok(true);
        };
        let Some(PendingWork::Resolution(cursor)) = self.pending_work.take() else {
            return Err(EffectError::WrongResolutionFrame);
        };
        let work = if let Some(source) = exploring {
            self.start_explore_step(cursor, player, source, card_id, extra_explores, append)?
        } else {
            LookTopWork {
                cursor,
                player,
                exploring: None,
                explore_nonland: false,
                remaining_explores: 0,
                domain: vec![card_id],
                selected: Vec::new(),
                decided: false,
                replacement: None,
            }
        };
        self.pending_work = Some(PendingWork::LookTop(Box::new(work)));
        Ok(true)
    }
    pub(super) fn bottom_remaining_looked_at(
        &mut self,
        player: SeatId,
        entry: StackEntryId,
        bottom: &[ObjectId],
    ) -> Result<(), EffectError> {
        if bottom.is_empty() {
            return Ok(());
        }
        let top = self
            .window
            .cards()
            .library(player)
            .get(..bottom.len())
            .ok_or(EffectError::InvalidLibraryPartition)?;
        let (window, _) = self.window.record_library_top(
            self.window.revision(),
            entry,
            crate::stack_window::LibraryTopCommit {
                player,
                frozen: top,
                top: &[],
                bottom,
                moves_application: &[],
            },
        )?;
        self.window = window;
        Ok(())
    }
    pub(super) fn top_remaining_looked_at(
        &mut self,
        player: SeatId,
        entry: StackEntryId,
        top_order: &[ObjectId],
    ) -> Result<(), EffectError> {
        if top_order.is_empty() {
            return Ok(());
        }
        let top = self
            .window
            .cards()
            .library(player)
            .get(..top_order.len())
            .ok_or(EffectError::InvalidLibraryPartition)?;
        let (window, _) = self.window.record_library_top(
            self.window.revision(),
            entry,
            crate::stack_window::LibraryTopCommit {
                player,
                frozen: top,
                top: top_order,
                bottom: &[],
                moves_application: &[],
            },
        )?;
        self.window = window;
        Ok(())
    }
    pub(super) fn hand_entry_selection(
        &self,
    ) -> Result<Option<ResolutionSelectionFrame>, EffectError> {
        let Instruction::ZoneOp(spec) = self.current_instruction()? else {
            return Err(EffectError::InconsistentProgram);
        };
        let crate::toolbox::ZoneDest::Delayed(
            crate::toolbox::DelayedZoneAction::PutHandPermanentsOntoBattlefield {
                player,
                types,
                maximum,
                ..
            },
        ) = &spec.destination
        else {
            return Err(EffectError::InconsistentProgram);
        };
        let chooser = self.resolving_player(*player)?;
        let maximum = usize::from(*maximum);
        let cursor = self
            .resolution_cursor()
            .ok_or(EffectError::WrongResolutionFrame)?;
        let evaluated = self.evaluated_characteristics()?;
        let domain: Vec<_> = evaluated
            .objects
            .values()
            .filter(|card| {
                card.zone == ObjectZone::Hand(chooser)
                    && !types.is_disjoint(&card.current_types)
                    && self
                        .linked_aura_domain(card.object, chooser, &evaluated)
                        .is_none_or(|candidates| !candidates.is_empty())
            })
            .map(|card| card.object)
            .collect();
        Ok((!domain.is_empty()).then(|| {
            ResolutionSelectionFrame::hand_entry(
                self.revision,
                cursor.entry,
                cursor.instruction,
                chooser,
                domain,
                maximum,
                ResolutionSelectionKind::HandToBattlefield,
            )
        }))
    }
    pub(super) fn resume_hand_entry(
        &self,
        expected: u64,
        frame: &ResolutionSelectionFrame,
        actor: SeatId,
        reply: DecisionReply,
    ) -> Result<RuleProgress, EffectError> {
        let revision = self.next_revision(expected)?;
        if self.hand_entry_selection()?.as_ref() != Some(frame) {
            return Err(EffectError::WrongResolutionFrame);
        }
        if actor != frame.chooser() {
            return Err(EffectError::WrongChooser);
        }
        let DecisionReply::SelectResolution(objects) = reply else {
            return Err(EffectError::WrongDecisionKind);
        };
        if objects.len() < frame.minimum()
            || objects.len() > frame.required()
            || objects.iter().collect::<BTreeSet<_>>().len() != objects.len()
            || objects.iter().any(|id| !frame.domain().contains(id))
        {
            return Err(EffectError::IllegalTarget);
        }
        let mut next = self.clone();
        let mut journal = RulesJournal::default();
        if !objects.is_empty() {
            if next.linked_return_work.is_some() {
                return Err(EffectError::InconsistentProgram);
            }
            next.linked_return_work = Some(LinkedReturnWork {
                origin: LinkedReturnOrigin::Hand {
                    tapped: matches!(
                        self.current_instruction()?,
                        Instruction::ZoneOp(spec)
                            if matches!(
                                spec.destination,
                                crate::toolbox::ZoneDest::Delayed(
                                    crate::toolbox::DelayedZoneAction::PutHandPermanentsOntoBattlefield {
                                        tapped: true,
                                        ..
                                    }
                                )
                            )
                    ),
                    player: actor,
                    entry: frame.entry(),
                    instruction: frame.instruction(),
                },
                objects,
                library_bottom: None,
                wake_entry: None,
                orders: BTreeMap::new(),
                graveyard_orders: BTreeMap::new(),
                choices: BTreeMap::new(),
                auras: BTreeMap::new(),
                replacements: None,
            });
            next.finish_linked_return_if_ready(&mut journal)?;
        }
        next.advance_instruction()?;
        next.revision = revision;
        next.progress_rules(
            DamageJournal::default(),
            vec![],
            vec![],
            DrawJournal::default(),
            journal,
        )
    }
    /// Whether these previously revealed library cards need an owner-provided graveyard order.
    pub fn is_looked_graveyard_order(&self, frame: &LinkedReturnOrderFrame) -> bool {
        self.look_top_frame() == Some(EffectDecision::LinkedReturnOrder(frame.clone()))
    }
    pub(super) fn resume_looked_order(
        &self,
        expected: u64,
        _bound: &LinkedReturnOrderFrame,
        _actor: SeatId,
        _reply: DecisionReply,
    ) -> Result<RuleProgress, EffectError> {
        let _ = self.next_revision(expected)?;
        Err(EffectError::WrongResolutionFrame)
    }
    fn look_top_batch(&self, work: &LookTopWork) -> Result<ZoneReplacementBatch, EffectError> {
        let (action, reason) = if work.explore_nonland {
            (
                ZoneActionKind::EffectRevealedToGraveyard,
                ZoneReason::RevealedToGraveyard,
            )
        } else {
            (ZoneActionKind::EffectLookToHand, ZoneReason::LookTopToHand)
        };
        self.begin_zone_replacements(zone_replacement::AuthorizedZoneBatch {
            cause: EventCause::Instruction {
                entry: work.cursor.entry,
                instruction: work.cursor.instruction,
            },
            action,
            intents: work
                .selected
                .iter()
                .map(|&before| zone_replacement::ZoneMoveIntent { before, reason })
                .collect(),
        })
    }
    pub(crate) fn resume_look_top(
        &self,
        expected: u64,
        frame: &LibraryTopFrame,
        actor: SeatId,
        selected: Vec<ObjectId>,
        tape: &crate::shuffle::EntropyTape,
    ) -> Result<(RuleProgress, crate::shuffle::EntropyTape), crate::game::GameError> {
        let revision = self.next_revision(expected)?;
        if self.look_top_frame().as_ref() != Some(&EffectDecision::LibraryTop(frame.clone())) {
            return Err(EffectError::WrongLibraryTopFrame.into());
        }
        if actor != frame.chooser() {
            return Err(EffectError::WrongChooser.into());
        }
        let mut next = self.clone();
        let Some(PendingWork::LookTop(work)) = next.pending_work.take() else {
            return Err(EffectError::WrongLibraryTopFrame.into());
        };
        let mut work = *work;
        if selected.len() > 1 {
            return Err(EffectError::InvalidLibraryPartition.into());
        }
        if selected.iter().any(|id| !work.domain.contains(id)) {
            return Err(EffectError::IllegalTarget.into());
        }
        if work.exploring.is_none() {
            let mut append = RulesJournal::default();
            work.cursor.results.revealed_top_types.clear();
            work.cursor.results.revealed_top_card = None;
            work.cursor.results.last_action_succeeded = !selected.is_empty();
            work.cursor.results.last_action_count = Some(RulesInteger::from_usize(selected.len()));
            if let Some(&id) = selected.first() {
                let before = next.rules_snapshot(None)?;
                let object = next
                    .window
                    .cards()
                    .get(id)
                    .cloned()
                    .ok_or(EffectError::UnknownObject)?;
                work.cursor.results.revealed_top_types = before
                    .view()
                    .evaluated
                    .get(id)
                    .ok_or(EffectError::InconsistentProgram)?
                    .current_types
                    .clone();
                work.cursor.results.revealed_top_card = Some(id);
                next.emit_from(
                    EventCause::Instruction {
                        entry: work.cursor.entry,
                        instruction: work.cursor.instruction,
                    },
                    &before,
                    None,
                    &mut append,
                    |_, _, _| {
                        Ok(vec![
                            crate::occurrence::RulesOccurrence::LibraryCardRevealed { object },
                        ])
                    },
                )?;
            }
            next.pending_work = Some(PendingWork::Resolution(work.cursor));
            next.revision = revision;
            next.advance_instruction()?;
            return Ok((
                next.progress_rules(
                    DamageJournal::default(),
                    vec![],
                    vec![],
                    DrawJournal::default(),
                    append,
                )?,
                tape.clone(),
            ));
        }
        work.selected = selected;
        work.decided = true;
        if !work.selected.is_empty() {
            work.replacement = Some(next.look_top_batch(&work)?);
        }
        next.revision = revision;
        next.pending_work = Some(PendingWork::LookTop(Box::new(work)));
        let progress = next.progress_look_top(
            DamageJournal::default(),
            vec![],
            vec![],
            DrawJournal::default(),
            RulesJournal::default(),
        )?;
        Ok((progress, tape.clone()))
    }
    pub(super) fn progress_look_top(
        mut self,
        journal: DamageJournal,
        declarations: Vec<CombatDeclarationRecord>,
        assignments: Vec<CombatAssignmentRecord>,
        draws: DrawJournal,
        mut append: RulesJournal,
    ) -> Result<RuleProgress, EffectError> {
        let Some(PendingWork::LookTop(work)) = self.pending_work.take() else {
            return Err(EffectError::WrongLibraryTopFrame);
        };
        let mut work = *work;
        if let Some(batch) = &mut work.replacement
            && let Some(frame) = batch.progress(self.revision)?
        {
            self.pending_work = Some(PendingWork::LookTop(Box::new(work)));
            return Ok(RuleProgress::new(
                self,
                RuleResult::Pending {
                    checks: CheckJournal::default(),
                    frame,
                    journal,
                    declarations,
                    assignments,
                    draws,
                },
                append,
            ));
        }
        let mut moves = Vec::new();
        if let Some(batch) = &work.replacement {
            batch.verify_initial(self.look_top_batch(&work)?)?;
            for &id in &work.selected {
                moves.push(crate::stack_window::LibraryTopMove {
                    before: id,
                    destination: batch.destination(id)?,
                });
            }
        }
        let before = self.rules_snapshot(None)?;
        let top = if work.explore_nonland {
            work.domain
                .iter()
                .copied()
                .filter(|id| !work.selected.contains(id))
                .collect()
        } else {
            Vec::new()
        };
        let (window, movements) = self.window.record_library_top(
            self.window.revision(),
            work.cursor.entry,
            crate::stack_window::LibraryTopCommit {
                player: work.player,
                frozen: &work.domain,
                top: &top,
                bottom: &[],
                moves_application: &moves,
            },
        )?;
        self.window = window;
        let reason = if work.explore_nonland {
            ZoneReason::RevealedToGraveyard
        } else {
            ZoneReason::LookTopToHand
        };
        if !movements.is_empty() {
            self.emit_from(
                EventCause::Instruction {
                    entry: work.cursor.entry,
                    instruction: work.cursor.instruction,
                },
                &before,
                None,
                &mut append,
                |previous, current, controllers| {
                    movements
                        .iter()
                        .map(|movement| {
                            zone_occurrence_at(previous, current, movement, reason, controllers)
                        })
                        .collect()
                },
            )?;
        }
        if let Some(source) = work.exploring {
            self.explored(source.clone(), work.player, &work.cursor, &mut append)?;
            if work.remaining_explores > 0
                && self
                    .window
                    .cards()
                    .get(source.id())
                    .is_some_and(|card| card.zone() == ObjectZone::Battlefield)
            {
                let remaining = work.remaining_explores - 1;
                if let Some(card_id) = self.window.cards().library(work.player).first().copied() {
                    let next_work = self.start_explore_step(
                        work.cursor,
                        work.player,
                        source,
                        card_id,
                        remaining,
                        &mut append,
                    )?;
                    let auto_commit = !next_work.explore_nonland;
                    self.pending_work = Some(PendingWork::LookTop(Box::new(next_work)));
                    if auto_commit {
                        return self.progress_look_top(
                            journal,
                            declarations,
                            assignments,
                            draws,
                            append,
                        );
                    }
                    let frame = self
                        .look_top_frame()
                        .ok_or(EffectError::WrongLibraryTopFrame)?;
                    return Ok(RuleProgress::new(
                        self,
                        RuleResult::Pending {
                            checks: CheckJournal::default(),
                            frame,
                            journal,
                            declarations,
                            assignments,
                            draws,
                        },
                        append,
                    ));
                }
                for _ in 0..=remaining {
                    self.explore_counter(&source, &work.cursor, &mut append)?;
                    self.explored(source.clone(), work.player, &work.cursor, &mut append)?;
                }
            }
        }
        self.pending_work = Some(PendingWork::Resolution(work.cursor));
        self.advance_instruction()?;
        self.progress_rules(journal, declarations, assignments, draws, append)
    }
}
