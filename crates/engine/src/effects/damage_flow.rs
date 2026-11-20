//! Shared batch continuation and one atomic physical result path.
use super::*;
use crate::{
    life::LifeError,
    occurrence::{EventCause, RulesOccurrence},
};
#[cfg(test)]
mod tests;

// Kept in the test module: this trusted primitive fixture deliberately exercises
// player recipients without broadening authored spell targeting or player inputs.
#[cfg(test)]
pub(crate) use tests::n04_player_batch_fixture;

pub(super) enum DamageProgressStep {
    Pending(DamageDecision),
    ResolutionContinued,
    CombatComplete,
}

impl EffectWindow {
    fn multiply_damage_at_cut(
        &mut self,
        application: DamageApplication,
        journal: &mut DamageJournal,
        append: &mut RulesJournal,
    ) -> Result<(), EffectError> {
        let before = self.rules_snapshot(None)?;
        let Some(PendingWork::Damage(work)) = &mut self.pending_work else {
            return Err(EffectError::WrongDamageFrame);
        };
        let batch = &mut work.batch;
        let multiplication = self.damage_state.apply_multiplier(batch, application)?;
        self.emit_from(
            EventCause::DamageAdjusted {
                batch: multiplication.batch,
            },
            &before,
            None,
            append,
            |_, _, _| {
                Ok(vec![RulesOccurrence::DamageMultiplied(
                    multiplication.clone(),
                )])
            },
        )?;
        journal.multiplications.push(multiplication);
        Ok(())
    }
    fn prevent_damage_at_cut(
        &mut self,
        application: DamageApplication,
        allocations: Vec<DamageAllocation>,
        journal: &mut DamageJournal,
        append: &mut RulesJournal,
    ) -> Result<(), EffectError> {
        let before = self.rules_snapshot(None)?;
        let DamageEffectId::Prevention(shield) = application.effect else {
            return Err(EffectError::WrongDamageFrame);
        };
        let shield_before = self
            .damage_state
            .shield(shield)
            .ok_or(EffectError::WrongDamageFrame)?
            .remaining
            .clone();
        let Some(PendingWork::Damage(work)) = &mut self.pending_work else {
            return Err(EffectError::WrongDamageFrame);
        };
        let batch = &mut work.batch;
        let damage_before = batch
            .components()
            .map(|component| (component.attempt().id, component.remaining().clone()))
            .collect();
        let prevention = self
            .damage_state
            .apply_prevention(batch, application, allocations)?;
        self.emit_from(
            EventCause::DamageAdjusted {
                batch: prevention.batch,
            },
            &before,
            None,
            append,
            |_, _, _| {
                Ok(vec![RulesOccurrence::DamagePrevented {
                    application: prevention.clone(),
                    damage_before,
                    shield_before,
                }])
            },
        )?;
        journal.preventions.push(prevention);
        Ok(())
    }
    pub(super) fn resolution_damage_source(
        &self,
        entry: &crate::stack_window::StackEntry,
    ) -> Result<DamageSource, EffectError> {
        match entry.kind() {
            StackKind::CardSpell(object) => {
                let object = self
                    .window
                    .cards()
                    .get(*object)
                    .ok_or(EffectError::UnknownObject)?;
                Ok(DamageSource::Object {
                    object: object.clone(),
                    controller: entry.controller(),
                })
            }
            StackKind::SpellCopy { .. } => Ok(DamageSource::SpellCopy {
                entry: entry.id(),
                controller: entry.controller(),
            }),
            StackKind::ActivatedAbility { source } | StackKind::TriggeredAbility { source } => {
                let current = self
                    .window
                    .cards()
                    .get(source.id())
                    .filter(|card| card.zone() == ObjectZone::Battlefield);
                let object = current.unwrap_or(source).clone();
                let controller = object
                    .controller()
                    .ok_or(EffectError::InconsistentProgram)?;
                Ok(DamageSource::Object { object, controller })
            }
        }
    }
    #[cfg(test)]
    pub(super) fn combat_damage_source(
        &self,
        object: ObjectId,
    ) -> Result<DamageSource, EffectError> {
        let evaluated = self.evaluated_characteristics()?;
        self.combat_damage_source_at(&evaluated, object)
    }
    /// Reuse one evaluated cut while collecting a whole combat damage batch.
    pub(super) fn combat_damage_source_at(
        &self,
        evaluated: &EvaluatedCharacteristics,
        object: ObjectId,
    ) -> Result<DamageSource, EffectError> {
        if !evaluated
            .get(object)
            .is_some_and(ObjectCharacteristics::is_creature)
        {
            return Err(CombatError::InvalidContext.into());
        }
        let object = self
            .window
            .cards()
            .get(object)
            .filter(|card| card.zone() == ObjectZone::Battlefield)
            .ok_or(EffectError::UnknownObject)?
            .clone();
        let controller = object.controller().ok_or(CombatError::InvalidContext)?;
        Ok(DamageSource::Object { object, controller })
    }
    fn validate_damage_origin_context(&self, origin: DamageOrigin) -> Result<(), EffectError> {
        if !self.state_based_losses.is_empty() {
            return Err(EffectError::PlayerLossPending);
        }
        if let Some(PendingWork::Damage(work)) = &self.pending_work {
            let parent_agrees = match (&work.parent, origin) {
                (
                    DamageParent::Resolution(cursor),
                    DamageOrigin::Resolution { entry, instruction },
                ) => cursor.entry == entry && cursor.instruction == instruction,
                (DamageParent::Combat, DamageOrigin::Combat { .. }) => true,
                _ => false,
            };
            if !parent_agrees || work.batch.origin() != origin {
                return Err(EffectError::WrongDamageFrame);
            }
        }
        match origin {
            DamageOrigin::Resolution { entry, instruction } => {
                if self.window.resolving().map(|e| e.id()) != Some(entry)
                    || !matches!(
                        self.payloads
                            .get(&entry)
                            .and_then(StackPayload::instructions)
                            .and_then(|p| p.get(instruction)),
                        Some(Instruction::Damage { .. })
                    )
                {
                    return Err(EffectError::WrongDamageFrame);
                }
            }
            DamageOrigin::Combat { position } => {
                if self.window.phase() != WindowPhase::TurnWork
                    || !self.window.entries().is_empty()
                    || self.combat.is_none()
                    || position.step != crate::turn::Step::CombatDamage
                    || position.additional_damage
                {
                    return Err(EffectError::WrongPhase);
                }
            }
        }
        Ok(())
    }
    pub(super) fn install_damage_batch(
        &mut self,
        origin: DamageOrigin,
        kind: DamageKind,
        inputs: Vec<DamageInput>,
        journal: &mut DamageJournal,
    ) -> Result<(), EffectError> {
        self.validate_damage_origin_context(origin)?;
        match origin {
            DamageOrigin::Resolution { entry, instruction } => {
                if kind != DamageKind::Noncombat {
                    return Err(EffectError::WrongPhase);
                }
                match &self.pending_work {
                    Some(PendingWork::Resolution(cursor))
                        if cursor.entry == entry && cursor.instruction == instruction => {}
                    Some(PendingWork::Resolution(_)) => return Err(EffectError::WrongDamageFrame),
                    _ => return Err(EffectError::WrongPhase),
                }
            }
            DamageOrigin::Combat { .. } => {
                if kind != DamageKind::Combat || self.pending_work.is_some() {
                    return Err(EffectError::WrongPhase);
                }
            }
        }
        let evaluated = self.evaluated_characteristics()?;
        for input in &inputs {
            self.damage_recipient_controller_at(&evaluated, input.target)?;
            if !self
                .window
                .participants()
                .contains(&input.source.controller())
            {
                return Err(EffectError::InconsistentProgram);
            }
        }
        let (batch, append) = self.damage_state.begin_batch(origin, kind, inputs)?;
        let parent = match self.pending_work.take() {
            Some(PendingWork::Resolution(cursor)) => DamageParent::Resolution(cursor),
            None => DamageParent::Combat,
            _ => return Err(EffectError::WrongDamageFrame),
        };
        self.pending_work = Some(PendingWork::Damage(DamageWork { batch, parent }));
        journal.append(append);
        Ok(())
    }
    /// Test-only direct mechanism fixture; production uses the shared driver.
    #[cfg(test)]
    pub(crate) fn begin_damage_batch(
        mut self,
        origin: DamageOrigin,
        kind: DamageKind,
        inputs: Vec<DamageInput>,
        journals: (DamageJournal, DrawJournal),
        declarations: Vec<CombatDeclarationRecord>,
        assignments: Vec<CombatAssignmentRecord>,
    ) -> Result<RuleProgress, EffectError> {
        let (mut journal, draws) = journals;
        self.install_damage_batch(origin, kind, inputs, &mut journal)?;
        self.progress_rules(
            journal,
            declarations,
            assignments,
            draws,
            RulesJournal::default(),
        )
    }
    fn damage_recipient_controller_at(
        &self,
        evaluated: &EvaluatedCharacteristics,
        target: DamageRecipient,
    ) -> Result<SeatId, EffectError> {
        match target {
            DamageRecipient::Creature(id) => {
                if !evaluated
                    .get(id)
                    .is_some_and(ObjectCharacteristics::is_creature)
                {
                    return Err(EffectError::IllegalTarget);
                }
                self.window
                    .cards()
                    .get(id)
                    .filter(|card| card.zone() == ObjectZone::Battlefield)
                    .and_then(|c| c.controller())
                    .ok_or(EffectError::IllegalTarget)
            }
            DamageRecipient::Player(player) => {
                if !self.window.participants().contains(&player) || self.life.get(player).is_none()
                {
                    return Err(EffectError::IllegalTarget);
                }
                Ok(player)
            }
        }
    }
    fn application_recipient(
        &self,
        batch: &ActiveDamageBatch,
        application: DamageApplication,
    ) -> Result<DamageRecipient, EffectError> {
        match application.scope {
            DamageScope::Recipient(recipient) => Ok(recipient),
            DamageScope::Component(id) => batch
                .component(id)
                .map(|c| c.attempt().target)
                .ok_or(EffectError::WrongDamageFrame),
        }
    }
    /// Fixed-recipient effects do not share capacity across objects. Process the
    /// next affected controller in APNAP order, then one independent recipient.
    fn next_damage_applications(
        &self,
        evaluated: &EvaluatedCharacteristics,
        batch: &ActiveDamageBatch,
    ) -> Result<(SeatId, Vec<DamageApplication>), EffectError> {
        let applications = self.damage_state.applicable(batch);
        let order = self.window.apnap_players()?.collect::<Vec<_>>();
        for actor in order {
            let mut recipients = BTreeMap::<DamageRecipient, Vec<DamageApplication>>::new();
            for application in &applications {
                let recipient = self.application_recipient(batch, *application)?;
                if self.damage_recipient_controller_at(evaluated, recipient)? == actor {
                    recipients.entry(recipient).or_default().push(*application);
                }
            }
            if let Some((_, applications)) = recipients.into_iter().next() {
                return Ok((actor, applications));
            }
        }
        Err(EffectError::WrongDamageFrame)
    }
    fn allocation_decision(
        &self,
        evaluated: &EvaluatedCharacteristics,
        batch: &ActiveDamageBatch,
        application: DamageApplication,
    ) -> Result<DamageDecision, EffectError> {
        let (shield, eligible, required) =
            self.damage_state.prevention_domain(batch, application)?;
        let recipient = self.application_recipient(batch, application)?;
        let chooser = self.damage_recipient_controller_at(evaluated, recipient)?;
        let eligible = eligible
            .into_iter()
            .map(|(attempt, amount)| {
                let component = batch
                    .component(attempt)
                    .ok_or(EffectError::WrongDamageFrame)?;
                Ok(DamageAllocationDomain {
                    attempt,
                    source: component.attempt().source.clone(),
                    amount,
                })
            })
            .collect::<Result<Vec<_>, EffectError>>()?;
        Ok(DamageDecision::Allocation(PreventionAllocationChoice {
            revision: self.revision,
            batch: batch.id(),
            stage: batch.stage(),
            origin: batch.origin(),
            chooser,
            application,
            capacity: shield.remaining.clone(),
            required,
            eligible,
        }))
    }
    fn current_damage_decision(&self) -> Result<DamageDecision, EffectError> {
        let batch = self.pending_damage().ok_or(EffectError::WrongDamageFrame)?;
        self.validate_damage_origin_context(batch.origin())?;
        let evaluated = self.evaluated_characteristics()?;
        match *batch.phase() {
            DamageBatchPhase::Allocating { application } => {
                self.allocation_decision(&evaluated, batch, application)
            }
            DamageBatchPhase::Selecting => {
                let (chooser, applications) = self.next_damage_applications(&evaluated, batch)?;
                if applications.len() < 2 {
                    return Err(EffectError::WrongDamageFrame);
                }
                Ok(DamageDecision::Order(DamageOrderChoice {
                    revision: self.revision,
                    batch: batch.id(),
                    stage: batch.stage(),
                    origin: batch.origin(),
                    chooser,
                    applications,
                }))
            }
        }
    }
    pub(super) fn resume_damage(
        &self,
        expected: u64,
        frame: &DamageDecision,
        actor: SeatId,
        reply: DecisionReply,
        turn: Option<&TurnScheduler>,
    ) -> Result<RuleProgress, EffectError> {
        if matches!(
            self.pending_work,
            Some(PendingWork::ManaColor(_) | PendingWork::Combat(_))
        ) {
            return Err(EffectError::WrongPhase);
        }
        let revision = self.next_revision(expected)?;
        if frame.revision() != self.revision || &self.current_damage_decision()? != frame {
            return Err(EffectError::WrongDamageFrame);
        }
        if actor != frame.chooser() {
            return Err(EffectError::WrongChooser);
        }
        if let DamageOrigin::Combat { position } = frame.origin() {
            let turn = turn.ok_or(EffectError::WrongPhase)?;
            self.require_combat_work(turn, TurnWork::CombatDamage { additional: false })?;
            if turn.position() != *position {
                return Err(EffectError::WrongDamageFrame);
            }
        }
        let mut next = self.clone();
        next.revision = revision;
        let mut journal = DamageJournal::default();
        let mut append = RulesJournal::default();
        match (frame, reply) {
            (DamageDecision::Order(bound), DecisionReply::DamageOrder(application)) => {
                if !bound.applications.contains(&application) {
                    return Err(DamageError::InapplicableEffect.into());
                }
                match application.effect {
                    DamageEffectId::Multiplier(_) => {
                        next.multiply_damage_at_cut(application, &mut journal, &mut append)?
                    }
                    DamageEffectId::Prevention(_) => {
                        let Some(PendingWork::Damage(work)) = &mut next.pending_work else {
                            return Err(EffectError::WrongDamageFrame);
                        };
                        next.damage_state
                            .select_prevention(&mut work.batch, application)?;
                    }
                }
            }
            (DamageDecision::Allocation(bound), DecisionReply::AllocatePrevention(allocations)) => {
                next.prevent_damage_at_cut(
                    bound.application,
                    allocations,
                    &mut journal,
                    &mut append,
                )?;
            }
            _ => return Err(EffectError::WrongDecisionKind),
        }
        next.progress_rules(journal, vec![], vec![], DrawJournal::default(), append)
    }
    pub(super) fn progress_damage(
        &mut self,
        journal: &mut DamageJournal,
        append: &mut RulesJournal,
    ) -> Result<DamageProgressStep, EffectError> {
        loop {
            let batch = self.pending_damage().ok_or(EffectError::WrongDamageFrame)?;
            self.validate_damage_origin_context(batch.origin())?;
            if let DamageBatchPhase::Allocating { application } = *batch.phase() {
                if let Some(allocations) =
                    self.damage_state.forced_allocation(batch, application)?
                {
                    self.prevent_damage_at_cut(application, allocations, journal, append)?;
                    continue;
                }
                let evaluated = self.evaluated_characteristics()?;
                let frame = self.allocation_decision(&evaluated, batch, application)?;
                return Ok(DamageProgressStep::Pending(frame));
            }
            if self.damage_state.applicable(batch).is_empty() {
                return self.commit_damage_results(journal, append);
            }
            let evaluated = self.evaluated_characteristics()?;
            let (chooser, applications) = self.next_damage_applications(&evaluated, batch)?;
            if applications.len() > 1 {
                return Ok(DamageProgressStep::Pending(DamageDecision::Order(
                    DamageOrderChoice {
                        revision: self.revision,
                        batch: batch.id(),
                        stage: batch.stage(),
                        origin: batch.origin(),
                        chooser,
                        applications,
                    },
                )));
            }
            let application = *applications.first().ok_or(EffectError::WrongDamageFrame)?;
            match application.effect {
                DamageEffectId::Multiplier(_) => {
                    self.multiply_damage_at_cut(application, journal, append)?
                }
                DamageEffectId::Prevention(_) => {
                    let Some(PendingWork::Damage(work)) = &mut self.pending_work else {
                        return Err(EffectError::WrongDamageFrame);
                    };
                    self.damage_state
                        .select_prevention(&mut work.batch, application)?;
                }
            }
        }
    }
    fn commit_damage_results(
        &mut self,
        journal: &mut DamageJournal,
        append: &mut RulesJournal,
    ) -> Result<DamageProgressStep, EffectError> {
        let batch = self.pending_damage().ok_or(EffectError::WrongDamageFrame)?;
        let origin = batch.origin();
        let kind = batch.kind();
        let id = batch.id();
        self.validate_damage_origin_context(origin)?;
        let next_instruction = match self.damage_parent().ok_or(EffectError::WrongDamageFrame)? {
            DamageParentView::Resolution(cursor) => Some(
                cursor
                    .instruction()
                    .checked_add(1)
                    .ok_or(EffectError::InconsistentProgram)?,
            ),
            DamageParentView::Combat => None,
        };
        let (applied, completions) = self.damage_state.finish_batch(batch)?;
        let before = self.rules_snapshot(None)?;
        let previous = before.view();
        let mut marks = BTreeMap::<ObjectId, RulesInteger>::new();
        let mut losses = BTreeMap::<SeatId, RulesInteger>::new();
        // Prepare and validate every result before the first physical write.
        for fact in &applied {
            self.damage_recipient_controller_at(&previous.evaluated, fact.target)?;
            match fact.target {
                DamageRecipient::Creature(id) => {
                    let amount = marks.entry(id).or_insert_with(|| 0.into());
                    *amount = amount.plus(&fact.amount);
                }
                DamageRecipient::Player(player) => {
                    let amount = losses.entry(player).or_insert_with(|| 0.into());
                    *amount = amount.plus(&fact.amount);
                }
            }
        }
        if !applied.is_empty() {
            self.emit_from(
                EventCause::DamageDealt { batch: id },
                &before,
                None,
                append,
                |_, _, _| {
                    Ok(applied
                        .iter()
                        .map(|damage| RulesOccurrence::DamageDealt {
                            damage: damage.clone(),
                            kind,
                            origin,
                        })
                        .collect())
                },
            )?;
        }
        let life_changes = self
            .life
            .apply_losses(&losses)
            .map_err(|error| match error {
                LifeError::UnknownPlayer => EffectError::IllegalTarget,
                LifeError::InvalidLoss | LifeError::InvalidAdjustment => {
                    EffectError::Damage(DamageError::InvalidAmount)
                }
            })?;
        for (&object, amount) in &marks {
            let previous = self
                .damage
                .get(&object)
                .cloned()
                .unwrap_or_else(|| 0.into());
            self.damage.insert(object, previous.plus(amount));
        }
        if !marks.is_empty() || !losses.is_empty() {
            self.emit_from(
                EventCause::DamageResults { batch: id },
                &before,
                None,
                append,
                |previous, current, _| {
                    let mut values: Vec<_> = life_changes
                        .iter()
                        .cloned()
                        .map(RulesOccurrence::LifeChanged)
                        .collect();
                    for &object in marks.keys() {
                        let recipient = DamageRecipient::Creature(object);
                        values.push(RulesOccurrence::DamageResult {
                            recipient,
                            before: previous
                                .marks
                                .get(&object)
                                .cloned()
                                .unwrap_or_else(|| 0.into()),
                            after: current
                                .marks
                                .get(&object)
                                .cloned()
                                .unwrap_or_else(|| 0.into()),
                            components: applied
                                .iter()
                                .filter(|fact| fact.target == recipient)
                                .map(|fact| fact.attempt)
                                .collect(),
                        });
                    }
                    for player in current
                        .window
                        .apnap_players()?
                        .filter(|player| losses.contains_key(player))
                    {
                        let recipient = DamageRecipient::Player(player);
                        values.push(RulesOccurrence::DamageResult {
                            recipient,
                            before: previous
                                .life
                                .get(player)
                                .ok_or(EffectError::InconsistentProgram)?
                                .clone(),
                            after: current
                                .life
                                .get(player)
                                .ok_or(EffectError::InconsistentProgram)?
                                .clone(),
                            components: applied
                                .iter()
                                .filter(|fact| fact.target == recipient)
                                .map(|fact| fact.attempt)
                                .collect(),
                        });
                    }
                    Ok(values)
                },
            )?;
        }
        journal.applied.extend(applied);
        journal.completions.extend(completions);
        let Some(PendingWork::Damage(work)) = self.pending_work.take() else {
            return Err(EffectError::WrongDamageFrame);
        };
        match work.parent {
            DamageParent::Resolution(mut cursor) => {
                cursor.instruction = next_instruction.ok_or(EffectError::WrongDamageFrame)?;
                self.pending_work = Some(PendingWork::Resolution(cursor));
                Ok(DamageProgressStep::ResolutionContinued)
            }
            DamageParent::Combat => {
                self.pending_work = None;
                Ok(DamageProgressStep::CombatComplete)
            }
        }
    }
}
