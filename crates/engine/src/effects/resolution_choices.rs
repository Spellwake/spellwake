//! Effect-time choices retain the one resolution cursor and the actual quote.
use super::*;
use crate::program::ResolvingPlayer;
mod chooser_replacement;
use chooser_replacement::ChooserSubstitution;
pub use chooser_replacement::ReplacementChooserFrame;
#[cfg(test)]
mod tests;

/// The selected outcome of one optional primitive, not trigger placement.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum OptionalEffectChoice {
    Take,
    Decline,
}

/// Resolution payment supplies only an allocation, never its payer or cost.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolutionPaymentReply {
    Pay(ManaPayment),
    Decline,
}

/// Explicit cost truth. A zero payment has a record even without a debit event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolutionPaymentOutcome {
    Paid(ManaPaymentRecord),
    Declined,
    /// CR 800.4f: the required payer already left; no choice or debit occurred.
    UnpaidDeparted,
}

/// Immutable executor-issued view of an optional primitive's actual domain.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OptionalEffectFrame {
    pub(crate) revision: u64,
    entry: StackEntryId,
    instruction: usize,
    chooser: SeatId,
    options: BTreeSet<OptionalEffectChoice>,
}
impl OptionalEffectFrame {
    /// Executor revision at which this domain was issued.
    pub fn revision(&self) -> u64 {
        self.revision
    }
    /// Actual resolving stack entry.
    pub fn entry(&self) -> StackEntryId {
        self.entry
    }
    /// Absolute index of the optional wrapper in that entry's body.
    pub fn instruction(&self) -> usize {
        self.instruction
    }
    /// Player derived by the owned instruction context.
    pub fn chooser(&self) -> SeatId {
        self.chooser
    }
    /// Offered choices after checking the next primitive's current availability.
    pub fn options(&self) -> &BTreeSet<OptionalEffectChoice> {
        &self.options
    }
}

/// Immutable current-revision quote. The owned work retains its continuation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolutionPaymentFrame {
    pub(crate) revision: u64,
    entry: StackEntryId,
    instruction: usize,
    payer: SeatId,
    cost: ManaCost,
}
impl ResolutionPaymentFrame {
    /// Latest executor revision, including accepted mana-ability prefixes.
    pub fn revision(&self) -> u64 {
        self.revision
    }
    /// Resolving offer entry, independently of its targeted spell or source card.
    pub fn entry(&self) -> StackEntryId {
        self.entry
    }
    /// Absolute index of the payment instruction in its owned body.
    pub fn instruction(&self) -> usize {
        self.instruction
    }
    /// The payer is also the only player authorized to answer this quote.
    pub fn chooser(&self) -> SeatId {
        self.payer
    }
    /// Player derived from the resolving instruction's actual context.
    pub fn payer(&self) -> SeatId {
        self.payer
    }
    /// Immutable authored cost retained by the owned payment work.
    pub fn cost(&self) -> &ManaCost {
        &self.cost
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct OptionalEffectWork {
    pub(super) cursor: ResolutionCursor,
    chooser: SeatId,
    options: BTreeSet<OptionalEffectChoice>,
    substitution: Option<ChooserSubstitution>,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ResolutionPaymentWork {
    pub(super) cursor: ResolutionCursor,
    pub(super) payer: SeatId,
    cost: ManaCost,
    paid_destination: usize,
    declined_destination: usize,
}
impl ResolutionPaymentWork {
    fn frame(&self, revision: u64) -> ResolutionPaymentFrame {
        ResolutionPaymentFrame {
            revision,
            entry: self.cursor.entry,
            instruction: self.cursor.instruction,
            payer: self.payer,
            cost: self.cost.clone(),
        }
    }
}

/// Private parent ownership never travels in the public ManaColorChoice token.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum ManaParent {
    Priority,
    Payment(ResolutionPaymentWork),
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ManaColorWork {
    pub(super) frame: ManaColorChoice,
    pub(super) parent: ManaParent,
}

/// Privileged borrowed continuation evidence; this cannot resume an instruction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolutionChoiceState<'a> {
    ChooserSubstitution {
        cursor: ResolutionState<'a>,
        departed: SeatId,
        controller: SeatId,
        candidates: &'a [SeatId],
        options: &'a BTreeSet<OptionalEffectChoice>,
    },
    Optional {
        cursor: ResolutionState<'a>,
        chooser: SeatId,
        options: &'a BTreeSet<OptionalEffectChoice>,
    },
    Payment {
        cursor: ResolutionState<'a>,
        payer: SeatId,
        cost: &'a ManaCost,
        paid_destination: usize,
        declined_destination: usize,
        suspended_for_mana_color: bool,
    },
}

/// Discriminant of the sole owned process; payload inspection stays borrowed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PendingWorkKind {
    LibraryTop,
    /// One cursor owns frozen choices, owner orders and a resolving batch.
    ResolutionSelection,
    Resolution,
    OptionalEffect,
    ResolutionPayment,
    ManaColor,
    TokenEntry,
    Damage,
    Checks,
    Combat,
}

impl EffectWindow {
    /// Observe which mutually exclusive process actually owns pending work.
    pub fn pending_work_kind(&self) -> Option<PendingWorkKind> {
        self.pending_work.as_ref().map(|work| match work {
            PendingWork::LibraryTop(_) => PendingWorkKind::LibraryTop,
            PendingWork::ResolutionSelection(_) => PendingWorkKind::ResolutionSelection,
            PendingWork::Resolution(_) => PendingWorkKind::Resolution,
            PendingWork::OptionalEffect(_) => PendingWorkKind::OptionalEffect,
            PendingWork::ResolutionPayment(_) => PendingWorkKind::ResolutionPayment,
            PendingWork::ManaColor(_) => PendingWorkKind::ManaColor,
            PendingWork::TokenEntry(_) => PendingWorkKind::TokenEntry,
            PendingWork::Damage(_) => PendingWorkKind::Damage,
            PendingWork::Checks(_) => PendingWorkKind::Checks,
            PendingWork::Combat(_) => PendingWorkKind::Combat,
        })
    }
    /// Current actual effect-time choice, derived at the latest executor revision.
    /// A color child exposes its own decision until its mana ability completes.
    pub fn pending_resolution_decision(&self) -> Option<EffectDecision> {
        match self.pending_work.as_ref() {
            Some(PendingWork::LibraryTop(_)) => self.library_top_frame(),
            Some(PendingWork::ResolutionSelection(_)) => self.selection_frame(),
            Some(PendingWork::OptionalEffect(work)) => {
                if let Some(frame) = work.replacement_frame(self.revision) {
                    return Some(EffectDecision::ReplacementChooser(frame));
                }
                Some(EffectDecision::OptionalEffect(OptionalEffectFrame {
                    revision: self.revision,
                    entry: work.cursor.entry,
                    instruction: work.cursor.instruction,
                    chooser: work.chooser,
                    options: work.options.clone(),
                }))
            }
            Some(PendingWork::ResolutionPayment(work)) => {
                Some(EffectDecision::ResolutionPayment(work.frame(self.revision)))
            }
            None
            | Some(
                PendingWork::TokenEntry(_)
                | PendingWork::Checks(_)
                | PendingWork::Combat(_)
                | PendingWork::Damage(_)
                | PendingWork::Resolution(_)
                | PendingWork::ManaColor(_),
            ) => None,
        }
    }

    /// Inspect the complete sole choice owner, including a suspended color parent.
    pub fn resolution_choice_state(&self) -> Option<ResolutionChoiceState<'_>> {
        let (work, suspended_for_mana_color) = match self.pending_work.as_ref() {
            Some(PendingWork::OptionalEffect(work)) => {
                if let Some(substitution) = &work.substitution {
                    return Some(ResolutionChoiceState::ChooserSubstitution {
                        cursor: (&work.cursor).into(),
                        departed: work.chooser,
                        controller: substitution.controller,
                        candidates: &substitution.candidates,
                        options: &work.options,
                    });
                }
                return Some(ResolutionChoiceState::Optional {
                    cursor: (&work.cursor).into(),
                    chooser: work.chooser,
                    options: &work.options,
                });
            }
            Some(PendingWork::ResolutionPayment(work)) => (work.as_ref(), false),
            Some(PendingWork::ManaColor(work)) => match &work.parent {
                ManaParent::Payment(parent) => (parent, true),
                ManaParent::Priority => return None,
            },
            None
            | Some(
                PendingWork::LibraryTop(_)
                | PendingWork::ResolutionSelection(_)
                | PendingWork::TokenEntry(_)
                | PendingWork::Checks(_)
                | PendingWork::Combat(_)
                | PendingWork::Damage(_)
                | PendingWork::Resolution(_),
            ) => return None,
        };
        Some(ResolutionChoiceState::Payment {
            cursor: (&work.cursor).into(),
            payer: work.payer,
            cost: &work.cost,
            paid_destination: work.paid_destination,
            declined_destination: work.declined_destination,
            suspended_for_mana_color,
        })
    }

    pub(super) fn resolving_player(&self, role: ResolvingPlayer) -> Result<SeatId, EffectError> {
        let player = self.resolving_player_identity(role)?;
        if !self.window.participants().contains(&player) {
            return Err(EffectError::InvalidResolvingPlayer);
        }
        Ok(player)
    }

    pub(super) fn resolving_player_identity(
        &self,
        role: ResolvingPlayer,
    ) -> Result<SeatId, EffectError> {
        let entry = self
            .window
            .resolving()
            .ok_or(EffectError::InvalidResolvingPlayer)?;
        let player = match role {
            ResolvingPlayer::Controller => entry.controller(),
            ResolvingPlayer::TargetPlayer => {
                let [bound] = entry.targets().targets() else {
                    return Err(EffectError::InvalidResolvingPlayer);
                };
                let TargetRef::Player(player) = bound.choice().target else {
                    return Err(EffectError::InvalidResolvingPlayer);
                };
                player
            }
            ResolvingPlayer::UniqueRelatedPlayer => {
                let binding = self
                    .execution_bindings(entry.id())
                    .and_then(|value| value.trigger.as_ref())
                    .ok_or(EffectError::InvalidResolvingPlayer)?;
                if !matches!(entry.kind(), StackKind::TriggeredAbility { source } if source == &binding.source.object)
                {
                    return Err(EffectError::InvalidResolvingPlayer);
                }
                let mut participants = BTreeSet::new();
                for cause in binding.causes.members() {
                    participants.insert(
                        cause
                            .related_player
                            .ok_or(EffectError::InvalidResolvingPlayer)?,
                    );
                }
                let mut actual = participants.into_iter();
                let player = actual.next().ok_or(EffectError::InvalidResolvingPlayer)?;
                if actual.next().is_some() {
                    return Err(EffectError::InvalidResolvingPlayer);
                }
                player
            }
            ResolvingPlayer::TargetSpellController => {
                let [bound] = entry.targets().targets() else {
                    return Err(EffectError::InvalidResolvingPlayer);
                };
                let TargetRef::Stack(target) = bound.choice().target else {
                    return Err(EffectError::InvalidResolvingPlayer);
                };
                self.window
                    .entries()
                    .iter()
                    .find(|candidate| {
                        candidate.id() == target
                            && matches!(
                                candidate.kind(),
                                StackKind::CardSpell(_) | StackKind::SpellCopy { .. }
                            )
                    })
                    .ok_or(EffectError::InvalidResolvingPlayer)?
                    .controller()
            }
        };
        if !self.window.known_player(player) {
            return Err(EffectError::InvalidResolvingPlayer);
        }
        Ok(player)
    }

    /// Availability is distinct from the driver's targeted-operation dispatch.
    fn optional_effect_available(&self, instruction: &Instruction) -> Result<bool, EffectError> {
        if !instruction.is_optional_primitive() {
            return Err(EffectError::InconsistentProgram);
        }
        match instruction {
            Instruction::Scry { .. }
            | Instruction::Surveil { .. }
            | Instruction::Mill { .. }
            | Instruction::Sacrifice { .. }
            | Instruction::Discard { .. } => Err(EffectError::InconsistentProgram),
            Instruction::DrawController { .. }
            | Instruction::AdjustControllerLife { .. }
            | Instruction::CreateTokens { .. } => Ok(true),
            Instruction::ChooseCreatureType { excluding, .. } => {
                Ok(excluding.len() < self.creature_vocabulary.len())
            }
            Instruction::CopyTargetToSource { .. } => {
                let entry = self
                    .window
                    .resolving()
                    .ok_or(EffectError::InconsistentProgram)?;
                let StackKind::ActivatedAbility { source } = entry.kind() else {
                    return Err(EffectError::InconsistentProgram);
                };
                if !self
                    .window
                    .cards()
                    .get(source.id())
                    .is_some_and(|object| object.zone() == ObjectZone::Battlefield)
                {
                    return Ok(false);
                }
                let [bound] = entry.targets().targets() else {
                    return Err(EffectError::InconsistentProgram);
                };
                let TargetRef::Object(donor) = bound.choice().target else {
                    return Err(EffectError::InconsistentProgram);
                };
                Ok(
                    self.operation_available(instruction, bound.choice().target)?
                        && self.copiable_program(donor).is_some_and(|program| {
                            matches!(program.kind, ProgramKind::Permanent(_))
                        }),
                )
            }
            Instruction::AdjustTargetLife { .. }
            | Instruction::Damage { .. }
            | Instruction::CreatePreventionShield { .. }
            | Instruction::CreateDamageMultiplier { .. }
            | Instruction::ReturnToOwnersHand
            | Instruction::CounterSpell
            | Instruction::CopySpell
            | Instruction::CreateContinuousEffect { .. }
            | Instruction::PutPowerToughnessCounters { .. } => {
                let entry = self
                    .window
                    .resolving()
                    .ok_or(EffectError::InconsistentProgram)?;
                let [bound] = entry.targets().targets() else {
                    return Err(EffectError::InconsistentProgram);
                };
                self.operation_available(instruction, bound.choice().target)
            }
            Instruction::OptionalEffect { .. }
            | Instruction::OfferManaPayment { .. }
            | Instruction::BranchIfFalse { .. }
            | Instruction::Jump { .. } => Err(EffectError::InconsistentProgram),
        }
    }

    fn optional_effect_options(
        &self,
        cursor: &ResolutionCursor,
    ) -> Result<BTreeSet<OptionalEffectChoice>, EffectError> {
        let following = cursor
            .instruction
            .checked_add(1)
            .ok_or(EffectError::InconsistentProgram)?;
        let operation = self
            .payloads
            .get(&cursor.entry)
            .and_then(StackPayload::instructions)
            .and_then(|instructions| instructions.get(following))
            .ok_or(EffectError::InconsistentProgram)?;
        let mut options = BTreeSet::from([OptionalEffectChoice::Decline]);
        if self.optional_effect_available(operation)? {
            options.insert(OptionalEffectChoice::Take);
        }
        Ok(options)
    }

    /// Refresh only admitted resolution-choice owners after the final cut.
    /// Effective optional choosers survive; unpaid costs restore their saved
    /// cursor. Ceased entries retain committed prefix effects without completing.
    pub(super) fn refresh_resolution_choice_after_departure(
        &mut self,
        before: &Self,
        turn: Option<&TurnScheduler>,
        append: &mut RulesJournal,
    ) -> Result<(), EffectError> {
        if matches!(self.pending_work, Some(PendingWork::Checks(_))) {
            return self.refresh_checks_after_departure(before, turn, append);
        }
        if matches!(self.pending_work, Some(PendingWork::ResolutionSelection(_))) {
            return self.refresh_selection_after_departure(before);
        }
        if matches!(self.pending_work, Some(PendingWork::LibraryTop(_))) {
            return self.refresh_library_after_departure(before);
        }
        if matches!(self.pending_work, Some(PendingWork::TokenEntry(_))) {
            return self.refresh_token_entries_after_departure();
        }
        if let Some(PendingWork::ManaColor(work)) = &self.pending_work {
            let mut work = work.clone();
            let chooser_live = self.window.participants().contains(&work.frame.chooser);
            match &work.parent {
                ManaParent::Priority => {
                    if !chooser_live {
                        // The sole mana recipient left. Retire the choice with
                        // no production and no invented stack-object receipt.
                        self.pending_work = None;
                        return Ok(());
                    }
                }
                ManaParent::Payment(parent) => {
                    if parent.payer != work.frame.chooser {
                        return Err(EffectError::InconsistentProgram);
                    }
                    let parent_survives = self.window.resolving().map(|entry| entry.id())
                        == Some(parent.cursor.entry);
                    if !parent_survives
                        && self
                            .window
                            .entries()
                            .iter()
                            .any(|entry| entry.id() == parent.cursor.entry)
                    {
                        return Err(EffectError::InconsistentProgram);
                    }
                    if !chooser_live {
                        if parent_survives {
                            self.finish_resolution_payment(
                                parent.clone(),
                                ResolutionPaymentOutcome::UnpaidDeparted,
                                append,
                            )?;
                        } else {
                            self.pending_work = None;
                        }
                        return Ok(());
                    }
                    if !parent_survives {
                        // The already paid mana ability still resolves for its
                        // live controller. Its dead offering entry cannot be a
                        // return address; settle current priority after color.
                        work.parent = ManaParent::Priority;
                    }
                }
            }
            work.frame.revision = self.revision;
            self.pending_work = Some(PendingWork::ManaColor(work));
            return Ok(());
        }
        let entry = match &self.pending_work {
            Some(PendingWork::OptionalEffect(work)) => work.cursor.entry,
            Some(PendingWork::ResolutionPayment(work)) => work.cursor.entry,
            Some(PendingWork::Resolution(cursor)) => cursor.entry,
            _ => return Ok(()),
        };
        if self.window.resolving().map(|entry| entry.id()) != Some(entry) {
            if self
                .window
                .entries()
                .iter()
                .any(|current| current.id() == entry)
            {
                return Err(EffectError::InconsistentProgram);
            }
            self.pending_work = None;
            return Ok(());
        }
        match &self.pending_work {
            Some(PendingWork::OptionalEffect(work)) => {
                let mut work = work.clone();
                work.substitution = self.chooser_substitution(work.chooser)?;
                work.options = self.optional_effect_options(&work.cursor)?;
                self.pending_work = Some(PendingWork::OptionalEffect(work));
            }
            Some(PendingWork::ResolutionPayment(work))
                if !self.window.participants().contains(&work.payer) =>
            {
                self.finish_resolution_payment(
                    work.as_ref().clone(),
                    ResolutionPaymentOutcome::UnpaidDeparted,
                    append,
                )?;
            }
            Some(PendingWork::ResolutionPayment(_) | PendingWork::Resolution(_)) => {}
            _ => return Err(EffectError::InconsistentProgram),
        }
        Ok(())
    }

    /// Continue only explicitly admitted resolution or check owners after departure.
    /// Accept every fact before root drive; begin_resolution would wrongly start
    /// the saved instruction at zero.
    pub(crate) fn progress_departure_work(
        &self,
        turn: Option<&TurnScheduler>,
    ) -> Result<RuleProgress, EffectError> {
        if let Some(PendingWork::Combat(cursor)) = &self.pending_work {
            let turn = turn.ok_or(CombatError::InvalidContext)?;
            // Blockers already declared by earlier defenders remain committed.
            let remaining = std::iter::once(cursor.frame.chooser())
                .chain(cursor.remaining_actors.iter().copied())
                .filter(|seat| self.window.participants().contains(seat))
                .collect();
            return match &cursor.frame {
                CombatDecision::Blockers(_) => self.clone().advance_blocker_actors(
                    remaining,
                    vec![],
                    turn,
                    RulesJournal::default(),
                ),
                CombatDecision::DamageAssignments(_) => {
                    self.resume_damage_after_departure(turn, &remaining, &cursor.assignments)
                }
                _ => Err(CombatError::WrongFrame.into()),
            };
        }
        if matches!(self.pending_work, Some(PendingWork::Checks(_))) {
            return self.progress_checks_after_departure(turn);
        }
        if !matches!(
            self.pending_work,
            Some(
                PendingWork::Resolution(_)
                    | PendingWork::TokenEntry(_)
                    | PendingWork::LibraryTop(_)
                    | PendingWork::ResolutionSelection(_)
            )
        ) {
            return Err(EffectError::WrongResolutionFrame);
        }
        self.clone().progress_rules(
            DamageJournal::default(),
            vec![],
            vec![],
            DrawJournal::default(),
            RulesJournal::default(),
        )
    }

    fn finish_resolution_payment(
        &mut self,
        work: ResolutionPaymentWork,
        outcome: ResolutionPaymentOutcome,
        append: &mut RulesJournal,
    ) -> Result<(), EffectError> {
        let destination = match &outcome {
            ResolutionPaymentOutcome::Paid(_) => work.paid_destination,
            ResolutionPaymentOutcome::Declined | ResolutionPaymentOutcome::UnpaidDeparted => {
                work.declined_destination
            }
        };
        append
            .records
            .push(crate::occurrence::RulesRecord::ResolutionPaymentOutcome {
                entry: work.cursor.entry,
                instruction: work.cursor.instruction,
                payer: work.payer,
                cost: work.cost,
                outcome,
                next: destination,
            });
        self.pending_work = Some(PendingWork::Resolution(work.cursor));
        self.set_instruction(destination)
    }

    pub(super) fn begin_resolution_choice(
        &mut self,
        instruction: &Instruction,
        append: &mut RulesJournal,
    ) -> Result<bool, EffectError> {
        match instruction {
            Instruction::OptionalEffect { chooser } => {
                let cursor = self
                    .resolution_cursor()
                    .ok_or(EffectError::WrongResolutionFrame)?
                    .clone();
                let chooser = self.resolving_player_identity(*chooser)?;
                let substitution = self.chooser_substitution(chooser)?;
                let options = self.optional_effect_options(&cursor)?;
                self.pending_work =
                    Some(PendingWork::OptionalEffect(Box::new(OptionalEffectWork {
                        cursor,
                        chooser,
                        options,
                        substitution,
                    })));
            }
            Instruction::OfferManaPayment {
                payer,
                cost,
                paid_destination,
                declined_destination,
            } => {
                let cursor = self
                    .resolution_cursor()
                    .ok_or(EffectError::WrongResolutionFrame)?
                    .clone();
                let payer = self.resolving_player_identity(*payer)?;
                let work = ResolutionPaymentWork {
                    cursor,
                    payer,
                    cost: cost.clone(),
                    paid_destination: *paid_destination,
                    declined_destination: *declined_destination,
                };
                if !self.window.participants().contains(&payer) {
                    self.finish_resolution_payment(
                        work,
                        ResolutionPaymentOutcome::UnpaidDeparted,
                        append,
                    )?;
                    return Ok(true);
                }
                self.pending_work = Some(PendingWork::ResolutionPayment(Box::new(work)));
            }
            Instruction::BranchIfFalse { .. }
            | Instruction::Jump { .. }
            | Instruction::Sacrifice { .. }
            | Instruction::Discard { .. }
            | Instruction::CreateTokens { .. }
            | Instruction::AdjustControllerLife { .. }
            | Instruction::AdjustTargetLife { .. }
            | Instruction::ChooseCreatureType { .. }
            | Instruction::Damage { .. }
            | Instruction::CreatePreventionShield { .. }
            | Instruction::CreateDamageMultiplier { .. }
            | Instruction::ReturnToOwnersHand
            | Instruction::CounterSpell
            | Instruction::CopySpell
            | Instruction::CopyTargetToSource { .. }
            | Instruction::CreateContinuousEffect { .. }
            | Instruction::PutPowerToughnessCounters { .. }
            | Instruction::Scry { .. }
            | Instruction::Surveil { .. }
            | Instruction::Mill { .. }
            | Instruction::DrawController { .. } => return Ok(false),
        }
        Ok(true)
    }

    pub(super) fn mana_activation_parent(&self, actor: SeatId) -> Result<ManaParent, EffectError> {
        if let Some(PendingWork::ResolutionPayment(work)) = &self.pending_work {
            if self.window.resolving().map(|entry| entry.id()) != Some(work.cursor.entry) {
                return Err(EffectError::WrongResolutionPaymentFrame);
            }
            if work.payer != actor {
                return Err(EffectError::WrongChooser);
            }
            return Ok(ManaParent::Payment(work.as_ref().clone()));
        }
        self.ensure_resolution_or_idle()?;
        self.window.validate_action_actor(actor)?;
        Ok(ManaParent::Priority)
    }

    pub(super) fn resume_optional_effect(
        &self,
        expected: u64,
        frame: &OptionalEffectFrame,
        actor: SeatId,
        reply: DecisionReply,
    ) -> Result<RuleProgress, EffectError> {
        let revision = self.next_revision(expected)?;
        if self.pending_resolution_decision() != Some(EffectDecision::OptionalEffect(frame.clone()))
        {
            return Err(EffectError::WrongOptionalEffectFrame);
        }
        if actor != frame.chooser {
            return Err(EffectError::WrongChooser);
        }
        let DecisionReply::OptionalEffect(choice) = reply else {
            return Err(EffectError::WrongDecisionKind);
        };
        if !frame.options.contains(&choice) {
            return Err(EffectError::IllegalOptionalEffect);
        }
        let mut next = self.clone();
        let Some(PendingWork::OptionalEffect(work)) = next.pending_work.take() else {
            return Err(EffectError::WrongOptionalEffectFrame);
        };
        let increment = match choice {
            OptionalEffectChoice::Take => 1,
            OptionalEffectChoice::Decline => 2,
        };
        let destination = work
            .cursor
            .instruction
            .checked_add(increment)
            .ok_or(EffectError::InconsistentProgram)?;
        next.revision = revision;
        next.pending_work = Some(PendingWork::Resolution(work.cursor));
        next.set_instruction(destination)?;
        let append = RulesJournal {
            records: vec![crate::occurrence::RulesRecord::OptionalEffectOutcome {
                entry: frame.entry,
                instruction: frame.instruction,
                chooser: actor,
                outcome: choice,
                next: destination,
            }],
        };
        next.progress_rules(
            DamageJournal::default(),
            vec![],
            vec![],
            DrawJournal::default(),
            append,
        )
    }

    pub(super) fn resume_resolution_payment(
        &self,
        expected: u64,
        frame: &ResolutionPaymentFrame,
        actor: SeatId,
        reply: DecisionReply,
        turn: Option<&TurnScheduler>,
    ) -> Result<RuleProgress, EffectError> {
        let revision = self.next_revision(expected)?;
        if self.pending_resolution_decision()
            != Some(EffectDecision::ResolutionPayment(frame.clone()))
        {
            return Err(EffectError::WrongResolutionPaymentFrame);
        }
        if actor != frame.payer {
            return Err(EffectError::WrongChooser);
        }
        let DecisionReply::ResolutionPayment(reply) = reply else {
            return Err(EffectError::WrongDecisionKind);
        };
        let mut next = self.clone();
        let Some(PendingWork::ResolutionPayment(work)) = next.pending_work.take() else {
            return Err(EffectError::WrongResolutionPaymentFrame);
        };
        let mut append = RulesJournal::default();
        let outcome = match reply {
            ResolutionPaymentReply::Pay(allocation) => {
                let spend = prepare_payment(&work.cost, Some(&allocation), |kind| {
                    self.mana(work.payer, kind)
                })
                .map_err(EffectError::InvalidManaPayment)?;
                let payment = ManaPaymentRecord {
                    source: work.cursor.entry,
                    payer: work.payer,
                    cost: work.cost.clone(),
                    spend,
                };
                next.debit_mana_at_cut(&payment, turn, &mut append)?;
                ResolutionPaymentOutcome::Paid(payment)
            }
            ResolutionPaymentReply::Decline => ResolutionPaymentOutcome::Declined,
        };
        next.revision = revision;
        next.finish_resolution_payment(*work, outcome, &mut append)?;
        next.progress_rules(
            DamageJournal::default(),
            vec![],
            vec![],
            DrawJournal::default(),
            append,
        )
    }
}
