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
        if !domain.trample_lethal().is_empty()
            && selected.iter().any(|row| {
                row.source == domain.source()
                    && matches!(
                        row.target,
                        DamageRecipient::Player(_)
                            | DamageRecipient::Planeswalker(_)
                            | DamageRecipient::Battle(_)
                    )
            })
        {
            for (&blocker, minimum) in domain.trample_lethal() {
                let assigned = selected.iter().find(|row| {
                    row.source == domain.source()
                        && row.target == DamageRecipient::Creature(blocker)
                });
                if minimum.is_positive() && assigned.is_none_or(|row| row.amount < *minimum) {
                    return Err(CombatError::InsufficientTrampleAssignment);
                }
            }
        }
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
        additional: bool,
    ) -> Result<Vec<DamageAssignmentDomain>, EffectError> {
        let combat = self.combat.as_ref().ok_or(CombatError::InvalidContext)?;
        let eligible = |source: Combatant| {
            let early = combat.first_strike_participants();
            // The second step uses the identity at the first step, even when an ability
            // was gained or lost during the intervening priority window.
            let had_first_strike = early.get(&source.object()) == Some(&source);
            let assigns_this_step = if early.is_empty() {
                true
            } else if additional {
                !had_first_strike
                    || evaluated
                        .get(source.object())
                        .is_some_and(|view| view.has_keyword(KeywordAbility::DoubleStrike))
            } else {
                had_first_strike
            };
            self.current_damage_combatant(evaluated, source) && assigns_this_step
        };
        let mut domains = Vec::new();
        for attacker in combat.attackers() {
            let source = attacker.combatant();
            let defender_present = attacker.defending_planeswalker().is_some()
                || self.window.participants().contains(&attacker.defender());
            let blocked_by_banding = crate::combat::controls_opposing_assignment(
                combat
                    .blockers()
                    .filter(|blocker| {
                        blocker.attackers().contains(&source.object())
                            && self.current_damage_combatant(evaluated, blocker.combatant())
                    })
                    .filter_map(|blocker| evaluated.get(blocker.combatant().object())),
                &self.name_rules,
            );
            let chooser = if blocked_by_banding {
                attacker.defender()
            } else {
                source.controller()
            };
            if chooser != actor || !eligible(source) {
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
            let mut lethal = BTreeMap::new();
            let mut recipients = Vec::new();
            match attacker.block_status() {
                BlockStatus::NotDeclared if !defender_present => continue,
                BlockStatus::NotDeclared => return Err(CombatError::InvalidContext.into()),
                BlockStatus::Unblocked => {
                    if !defender_present {
                        continue;
                    }
                    recipients.push(attacker.defending_planeswalker().map_or(
                        DamageRecipient::Player(attacker.defender()),
                        |id| {
                            if evaluated
                                .get(id)
                                .is_some_and(|view| view.has_type(crate::program::CardType::Battle))
                            {
                                DamageRecipient::Battle(id)
                            } else {
                                DamageRecipient::Planeswalker(id)
                            }
                        },
                    ))
                }
                BlockStatus::Blocked => {
                    let current = evaluated
                        .get(source.object())
                        .ok_or(CombatError::InvalidContext)?;
                    let trample = current.has_keyword(KeywordAbility::Trample);
                    for blocker in combat.blockers().filter(|blocker| {
                        blocker.attackers().contains(&source.object())
                            && self.current_damage_combatant(evaluated, blocker.combatant())
                    }) {
                        let id = blocker.combatant().object();
                        recipients.push(DamageRecipient::Creature(id));
                        if trample {
                            let threshold = evaluated
                                .get(id)
                                .and_then(|view| evaluated.lethal_damage_threshold(view))
                                .ok_or(CombatError::InvalidContext)?;
                            let marked = self.damage.get(&id).cloned().unwrap_or_else(|| 0.into());
                            let remaining = threshold.minus(&marked).max(0.into());
                            lethal.insert(
                                id,
                                if current.has_keyword(KeywordAbility::Deathtouch) {
                                    remaining.min(1.into())
                                } else {
                                    remaining
                                },
                            );
                        }
                    }
                    if trample && defender_present {
                        let required = lethal
                            .values()
                            .fold(RulesInteger::from(0), |sum, amount| sum.plus(amount));
                        if power > required {
                            recipients.push(attacker.defending_planeswalker().map_or(
                                DamageRecipient::Player(attacker.defender()),
                                |id| {
                                    if evaluated.get(id).is_some_and(|view| {
                                        view.has_type(crate::program::CardType::Battle)
                                    }) {
                                        DamageRecipient::Battle(id)
                                    } else {
                                        DamageRecipient::Planeswalker(id)
                                    }
                                },
                            ));
                        } else {
                            lethal.clear();
                        }
                    } else {
                        lethal.clear();
                    }
                }
            }
            if !recipients.is_empty() {
                domains.push(
                    DamageAssignmentDomain::new(source.object(), power, recipients)
                        .with_trample_lethal(lethal),
                );
            }
        }
        for blocker in combat.blockers() {
            let source = blocker.combatant();
            let blocking_banding = crate::combat::controls_opposing_assignment(
                blocker
                    .attackers()
                    .iter()
                    .filter(|id| {
                        combat.attacker(**id).is_some_and(|attacker| {
                            self.current_damage_combatant(evaluated, attacker.combatant())
                        })
                    })
                    .filter_map(|id| evaluated.get(*id)),
                &self.name_rules,
            );
            let chooser = if blocking_banding {
                self.window.active_player()
            } else {
                source.controller()
            };
            if chooser != actor || !eligible(source) {
                continue;
            }
            let recipients = blocker
                .attackers()
                .iter()
                .filter_map(|id| combat.attacker(*id))
                .filter(|attacker| self.current_damage_combatant(evaluated, attacker.combatant()))
                .map(|attacker| DamageRecipient::Creature(attacker.combatant().object()))
                .collect::<Vec<_>>();
            if recipients.is_empty() {
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
                    recipients,
                ));
            }
        }
        domains.sort_by_key(DamageAssignmentDomain::source);
        Ok(domains)
    }

    pub(super) fn require_damage_work(&self, turn: &TurnScheduler) -> Result<(), EffectError> {
        let additional = turn.position().additional_damage;
        self.require_combat_work(turn, TurnWork::CombatDamage { additional })?;
        let combat = self.combat.as_ref().ok_or(CombatError::InvalidContext)?;
        if additional && combat.first_strike_participants().is_empty() {
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
        self.require_damage_work(turn)?;
        if self.pending_work.is_some() {
            return Err(EffectError::WrongPhase);
        }
        let actors = self.window.apnap_players()?.collect();
        let mut next = self.clone();
        next.revision = revision;
        if !turn.position().additional_damage {
            let evaluated = self.evaluated_characteristics()?;
            let combat = next.combat.as_mut().ok_or(CombatError::InvalidContext)?;
            let participants = combat
                .attackers()
                .map(|row| row.combatant())
                .chain(combat.blockers().map(|row| row.combatant()))
                .filter(|&source| {
                    self.current_damage_combatant(&evaluated, source)
                        && evaluated.get(source.object()).is_some_and(|view| {
                            view.has_keyword(KeywordAbility::FirstStrike)
                                || view.has_keyword(KeywordAbility::DoubleStrike)
                        })
                })
                .map(|source| (source.object(), source))
                .collect();
            combat.begin_first_strike(participants);
        }
        next.advance_damage_assignment_actors(turn, actors, vec![], vec![], RulesJournal::default())
    }

    fn advance_damage_assignment_actors(
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
            let domains =
                self.combat_damage_domains(&evaluated, actor, turn.position().additional_damage)?;
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
                    attack_payment: None,
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
            let additional_step_required = self.additional_damage_required(position);
            return Ok(RuleProgress::new(
                self,
                RuleResult::Complete {
                    checks: CheckJournal::default(),
                    completion: RuleCompletion::TurnWork(WorkCompleted::CombatDamage {
                        additional_step_required,
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

    pub(super) fn additional_damage_required(&self, position: crate::turn::TurnPosition) -> bool {
        !position.additional_damage
            && self
                .combat
                .as_ref()
                .is_some_and(|combat| !combat.first_strike_participants().is_empty())
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
        if calendar.stage()
            != TurnStage::Work(TurnWork::CombatDamage {
                additional: calendar.position().additional_damage,
            })
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
        self.require_damage_work(turn)?;
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
            self.combat_damage_domains(&evaluated, actor, turn.position().additional_damage)?,
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
