//! Current-rule ordinary assignments produce the shared simultaneous batch.
use super::*;
use crate::{
    combat::{
        AssignmentOrigin, BlockStatus, CombatAssignmentRecord, CombatDamageAssignment, Combatant,
        DamageAssignmentDomain, DamageAssignmentsDecision,
    },
    damage::{DamageInput, DamageKind, DamageOrigin, DamageRecipient},
    occurrence::{EventCause, RulesJournal, RulesOccurrence},
    turn::{TurnStage, TurnWork, WorkCompleted},
};

#[cfg(test)]
mod occurrence_tests;

/// Validate the whole actor's sparse allocation before accepting any row.
fn validate_assignments(
    domains: &[DamageAssignmentDomain],
    mut selected: Vec<CombatDamageAssignment>,
) -> Result<Vec<CombatDamageAssignment>, CombatError> {
    let mut pairs = BTreeSet::new();
    let mut totals = BTreeMap::<ObjectId, RulesInteger>::new();
    for row in &selected {
        if !row.amount.is_positive() {
            return Err(CombatError::InvalidDamageAmount);
        }
        let domain = domains
            .iter()
            .find(|domain| domain.source() == row.source)
            .ok_or(CombatError::IllegalDamageSource)?;
        if !domain.recipients().contains(&row.target) {
            return Err(CombatError::IllegalDamageRecipient);
        }
        if !pairs.insert((row.source, row.target)) {
            return Err(CombatError::DuplicateAssignment);
        }
        let total = totals.entry(row.source).or_insert_with(|| 0.into());
        *total = total.plus(&row.amount);
    }
    for domain in domains {
        if totals.get(&domain.source()) != Some(domain.power()) {
            return Err(CombatError::IncorrectDamageTotal);
        }
    }
    selected.sort_by_key(|row| (row.source, row.target));
    Ok(selected)
}

impl EffectWindow {
    fn current_damage_combatant(
        &self,
        evaluated: &EvaluatedCharacteristics,
        combatant: Combatant,
    ) -> bool {
        self.window
            .cards()
            .get(combatant.object())
            .is_some_and(|card| {
                card.zone() == ObjectZone::Battlefield
                    && card.controller() == Some(combatant.controller())
                    && card.control_epoch() == Some(combatant.control_epoch())
                    && evaluated
                        .get(card.id())
                        .is_some_and(ObjectCharacteristics::is_creature)
            })
    }

    fn combat_damage_domains(
        &self,
        evaluated: &EvaluatedCharacteristics,
        actor: SeatId,
    ) -> Result<Vec<DamageAssignmentDomain>, EffectError> {
        let combat = self.combat.as_ref().ok_or(CombatError::InvalidContext)?;
        let mut domains = Vec::new();
        for attacker in combat.attackers() {
            let source = attacker.combatant();
            if source.controller() != actor || !self.current_damage_combatant(evaluated, source) {
                continue;
            }
            let power = evaluated
                .get(source.object())
                .and_then(|current| current.power_toughness.as_ref())
                .ok_or(CombatError::InvalidContext)?
                .derived
                .power
                .clone();
            if !power.is_positive() {
                continue;
            }
            let recipients = match attacker.block_status() {
                BlockStatus::NotDeclared => return Err(CombatError::InvalidContext.into()),
                // Rule 800.4e leaves damage to a departed defender unassigned.
                BlockStatus::Unblocked
                    if !self.window.participants().contains(&attacker.defender()) =>
                {
                    continue;
                }
                BlockStatus::Unblocked => vec![DamageRecipient::Player(attacker.defender())],
                BlockStatus::Blocked => combat
                    .blockers()
                    .filter(|blocker| {
                        blocker.attacker() == Some(source.object())
                            && self.current_damage_combatant(evaluated, blocker.combatant())
                    })
                    .map(|blocker| DamageRecipient::Creature(blocker.combatant().object()))
                    .collect(),
            };
            if !recipients.is_empty() {
                domains.push(DamageAssignmentDomain::new(
                    source.object(),
                    power,
                    recipients,
                ));
            }
        }
        for blocker in combat.blockers() {
            let source = blocker.combatant();
            if source.controller() != actor || !self.current_damage_combatant(evaluated, source) {
                continue;
            }
            let Some(attacker) = blocker.attacker().and_then(|id| combat.attacker(id)) else {
                continue;
            };
            if !self.current_damage_combatant(evaluated, attacker.combatant()) {
                continue;
            }
            let power = evaluated
                .get(source.object())
                .and_then(|current| current.power_toughness.as_ref())
                .ok_or(CombatError::InvalidContext)?
                .derived
                .power
                .clone();
            if power.is_positive() {
                domains.push(DamageAssignmentDomain::new(
                    source.object(),
                    power,
                    vec![DamageRecipient::Creature(attacker.combatant().object())],
                ));
            }
        }
        domains.sort_by_key(DamageAssignmentDomain::source);
        Ok(domains)
    }

    fn require_ordinary_damage_work(&self, turn: &TurnScheduler) -> Result<(), EffectError> {
        self.require_combat_work(turn, TurnWork::CombatDamage { additional: false })?;
        if turn.position().additional_damage || self.combat.is_none() {
            return Err(CombatError::InvalidContext.into());
        }
        Ok(())
    }

    pub(crate) fn begin_combat_damage(
        &self,
        expected: u64,
        turn: &TurnScheduler,
    ) -> Result<RuleProgress, EffectError> {
        let revision = self.next_revision(expected)?;
        self.require_ordinary_damage_work(turn)?;
        if self.pending_work.is_some() {
            return Err(EffectError::WrongPhase);
        }
        let actors = self.window.apnap_players()?.collect();
        let mut next = self.clone();
        next.revision = revision;
        next.advance_damage_assignment_actors(turn, actors, vec![], vec![], RulesJournal::default())
    }

    /// Preserve legal allocations by earlier actors. Reopen only actors whose
    /// allocations can no longer cover their surviving domains after departure.
    pub(super) fn resume_damage_after_departure(
        &self,
        turn: &TurnScheduler,
        remaining: &[SeatId],
        previous: &[CombatDamageAssignment],
    ) -> Result<RuleProgress, EffectError> {
        let evaluated = self.evaluated_characteristics()?;
        let mut selected = Vec::new();
        let mut actors = Vec::new();
        for actor in self.window.apnap_players()? {
            if remaining.contains(&actor) {
                actors.push(actor);
                continue;
            }
            let domains = self.combat_damage_domains(&evaluated, actor)?;
            let rows = previous
                .iter()
                .filter(|row| domains.iter().any(|domain| domain.source() == row.source))
                .cloned()
                .collect();
            match validate_assignments(&domains, rows) {
                Ok(rows) => selected.extend(rows),
                Err(_) => actors.push(actor),
            }
        }
        self.clone().advance_damage_assignment_actors(
            turn,
            actors,
            selected,
            vec![],
            RulesJournal::default(),
        )
    }

    pub(super) fn advance_damage_assignment_actors(
        mut self,
        turn: &TurnScheduler,
        mut actors: Vec<SeatId>,
        mut selected: Vec<CombatDamageAssignment>,
        mut records: Vec<CombatAssignmentRecord>,
        mut append: RulesJournal,
    ) -> Result<RuleProgress, EffectError> {
        let position = turn.position();
        self.pending_work = None;
        let evaluated = self.evaluated_characteristics()?;
        while !actors.is_empty() {
            let actor = actors.remove(0);
            let domains = self.combat_damage_domains(&evaluated, actor)?;
            if domains.is_empty() {
                continue;
            }
            if domains.iter().any(|domain| domain.recipients().len() > 1) {
                let frame = CombatDecision::DamageAssignments(DamageAssignmentsDecision::new(
                    self.revision,
                    actor,
                    position,
                    domains,
                ));
                self.pending_work = Some(PendingWork::Combat(CombatCursor {
                    frame: frame.clone(),
                    remaining_actors: actors,
                    assignments: selected,
                }));
                return Ok(RuleProgress::new(
                    self,
                    RuleResult::Pending {
                        checks: CheckJournal::default(),
                        frame: EffectDecision::Combat(frame),
                        journal: DamageJournal::default(),
                        declarations: vec![],
                        assignments: records,
                        draws: DrawJournal::default(),
                    },
                    append,
                ));
            }
            let assignments: Vec<_> = domains
                .into_iter()
                .map(|domain| CombatDamageAssignment {
                    source: domain.source(),
                    target: domain.recipients()[0],
                    amount: domain.power().clone(),
                })
                .collect();
            selected.extend(assignments.iter().cloned());
            self.emit_combat_assignments(
                turn,
                actor,
                AssignmentOrigin::Forced,
                &assignments,
                &mut append,
            )?;
            records.push(CombatAssignmentRecord {
                player: actor,
                origin: AssignmentOrigin::Forced,
                assignments,
            });
        }
        if selected.is_empty() {
            return Ok(RuleProgress::new(
                self,
                RuleResult::Complete {
                    checks: CheckJournal::default(),
                    completion: RuleCompletion::TurnWork(WorkCompleted::CombatDamage {
                        additional_step_required: false,
                    }),
                    journal: DamageJournal::default(),
                    declarations: vec![],
                    assignments: records,
                    permanent_entry: None,
                    draws: DrawJournal::default(),
                },
                append,
            ));
        }
        let inputs = selected
            .into_iter()
            .map(|row| {
                Ok(DamageInput {
                    source: self.combat_damage_source_at(&evaluated, row.source)?,
                    target: row.target,
                    amount: row.amount,
                })
            })
            .collect::<Result<Vec<_>, EffectError>>()?;
        let mut journal = DamageJournal::default();
        self.install_damage_batch(
            DamageOrigin::Combat { position },
            DamageKind::Combat,
            inputs,
            &mut journal,
        )?;
        self.progress_rules(journal, vec![], records, DrawJournal::default(), append)
    }

    fn emit_combat_assignments(
        &mut self,
        turn: &TurnScheduler,
        actor: SeatId,
        origin: AssignmentOrigin,
        assignments: &[CombatDamageAssignment],
        append: &mut RulesJournal,
    ) -> Result<(), EffectError> {
        if assignments.is_empty() {
            return Ok(());
        }
        let before = self.rules_snapshot(Some(turn))?;
        let view = before.view();
        let calendar = view.turn.ok_or(CombatError::InvalidContext)?;
        if calendar.stage() != TurnStage::Work(TurnWork::CombatDamage { additional: false })
            || view.combat.is_none()
        {
            return Err(CombatError::InvalidContext.into());
        }
        self.emit_from(
            EventCause::CombatDamageAssigned {
                position: calendar.position(),
                actor,
                origin,
            },
            &before,
            Some(turn),
            append,
            |_, _, _| {
                Ok(assignments
                    .iter()
                    .cloned()
                    .map(RulesOccurrence::CombatDamageAssigned)
                    .collect())
            },
        )
    }

    pub(super) fn resume_combat_damage_assignments(
        &self,
        expected: u64,
        frame: &DamageAssignmentsDecision,
        actor: SeatId,
        rows: Vec<CombatDamageAssignment>,
        turn: &TurnScheduler,
    ) -> Result<RuleProgress, EffectError> {
        let revision = self.next_revision(expected)?;
        self.require_ordinary_damage_work(turn)?;
        let Some(PendingWork::Combat(cursor)) = self.pending_work.as_ref() else {
            return Err(CombatError::WrongFrame.into());
        };
        if cursor.frame != CombatDecision::DamageAssignments(frame.clone()) {
            return Err(CombatError::WrongFrame.into());
        }
        if actor != frame.chooser() {
            return Err(EffectError::WrongChooser);
        }
        let evaluated = self.evaluated_characteristics()?;
        let reconstructed = DamageAssignmentsDecision::new(
            self.revision,
            actor,
            turn.position(),
            self.combat_damage_domains(&evaluated, actor)?,
        );
        if &reconstructed != frame {
            return Err(CombatError::WrongFrame.into());
        }
        let assignments = validate_assignments(frame.domains(), rows)?;
        let mut selected = cursor.assignments.clone();
        selected.extend(assignments.iter().cloned());
        let remaining = cursor.remaining_actors.clone();
        let mut next = self.clone();
        next.revision = revision;
        let mut append = RulesJournal::default();
        next.emit_combat_assignments(
            turn,
            actor,
            AssignmentOrigin::PlayerReply,
            &assignments,
            &mut append,
        )?;
        next.advance_damage_assignment_actors(
            turn,
            remaining,
            selected,
            vec![CombatAssignmentRecord {
                player: actor,
                origin: AssignmentOrigin::PlayerReply,
                assignments,
            }],
            append,
        )
    }
}
