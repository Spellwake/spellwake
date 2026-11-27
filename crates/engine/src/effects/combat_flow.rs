//! Declaration work under the existing executor/window/registry owner.
use super::*;
use crate::{
    combat::{
        AttackTap, AttackersDecision, BlockersDecision, CombatDecision, CombatDeclarationRecord,
        Combatant, DeclarationOrigin,
    },
    object_identity::GameObject,
    occurrence::{EventCause, RulesJournal, RulesOccurrence, StatusReason},
    turn::{TurnStage, TurnWork, WorkCompleted},
};

mod blocking;
#[cfg(test)]
mod occurrence_tests;

/// Allocation positions before one accepted input. Retained actions keep their original identities.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct AttackAllocationCut {
    window: [u64; 2],
    objects: [u64; 2],
    effects: [u64; 4],
    triggers: [u64; 3],
    damage: [u64; 4],
    mana_lot: u64,
}

impl EffectWindow {
    pub(crate) fn restore_attack_revision(&mut self, issued: u64) -> Result<(), EffectError> {
        if !self.is_paying_attack_cost() {
            return Err(CombatError::WrongFrame.into());
        }
        self.revision = self.revision.max(issued);
        Ok(())
    }
    pub(crate) fn attack_allocation_cut(&self) -> AttackAllocationCut {
        AttackAllocationCut {
            window: self.window.attack_allocation_mark(),
            objects: self.window.cards().attack_allocation_mark(),
            effects: self.characteristics.attack_allocation_mark(),
            triggers: self.triggers.attack_allocation_mark(),
            damage: self.damage_state.attack_allocation_mark(),
            mana_lot: self.floating_mana.next_id,
        }
    }
    pub(crate) fn retain_attack_allocation_cut(&mut self, mark: &AttackAllocationCut) {
        self.window.retain_attack_mark(mark.window);
        self.window.retain_attack_object_mark(mark.objects);
        self.characteristics.retain_attack_mark(mark.effects);
        self.triggers.retain_attack_mark(mark.triggers);
        self.damage_state.retain_attack_mark(mark.damage);
        self.floating_mana.next_id = self.floating_mana.next_id.max(mark.mana_lot);
    }

    pub fn combat_rules(&self) -> &CombatRules {
        &self.combat_rules
    }
    pub fn combat(&self) -> Option<&CombatState> {
        self.combat.as_ref()
    }

    pub(super) fn require_combat_work(
        &self,
        turn: &TurnScheduler,
        work: TurnWork,
    ) -> Result<(), EffectError> {
        if !self.state_based_losses.is_empty() {
            return Err(EffectError::PlayerLossPending);
        }
        if turn.stage() != TurnStage::Work(work)
            || self.window.phase() != WindowPhase::TurnWork
            || !self.window.entries().is_empty()
        {
            return Err(EffectError::WrongPhase);
        }
        Ok(())
    }

    pub(super) fn resolve_keyword_trigger(
        &mut self,
        keyword: &crate::program::CombatTriggerKeyword,
        entry: StackEntryId,
        instruction: usize,
        append: &mut RulesJournal,
    ) -> Result<(), EffectError> {
        use crate::characteristics::PowerToughness;
        use crate::program::CombatTriggerKeyword;
        let trigger = self
            .execution_bindings(entry)
            .and_then(|bindings| bindings.trigger.as_ref())
            .ok_or(EffectError::InconsistentProgram)?;
        let source = trigger.source.object.id();
        let (mut affected, power, toughness) = match keyword {
            CombatTriggerKeyword::Afflict(amount) => {
                let [cause] = trigger.causes.members() else {
                    return Err(EffectError::InconsistentProgram);
                };
                let defender = cause
                    .related_player
                    .ok_or(EffectError::InvalidResolvingPlayer)?;
                return self.adjust_life_at_cut(
                    entry,
                    instruction,
                    defender,
                    &crate::life::LifeAdjustment::Lose(amount.clone()),
                    append,
                );
            }
            CombatTriggerKeyword::Melee => {
                let amount = RulesInteger::from_usize(
                    self.combat
                        .as_ref()
                        .map_or(0, crate::combat::CombatState::attacked_player_count),
                );
                (BTreeSet::from([source]), amount.clone(), amount)
            }
            CombatTriggerKeyword::Dethrone
            | CombatTriggerKeyword::Training
            | CombatTriggerKeyword::Renown(_) => {
                if !self
                    .window
                    .cards()
                    .get(source)
                    .is_some_and(|card| card.zone() == ObjectZone::Battlefield)
                {
                    return Ok(());
                }
                let (amount, placement) = if let CombatTriggerKeyword::Renown(amount) = keyword {
                    if self.characteristics.renowned.contains(&source) {
                        return Ok(());
                    }
                    (
                        amount.clone(),
                        super::resolution_choices::CounterPlacement::Renown,
                    )
                } else {
                    (
                        1.into(),
                        super::resolution_choices::CounterPlacement::Ordinary,
                    )
                };
                return self.add_source_counters(
                    source,
                    entry,
                    instruction,
                    amount,
                    placement,
                    append,
                );
            }
            CombatTriggerKeyword::Decayed => {
                let controller = self
                    .window
                    .resolving()
                    .ok_or(EffectError::WrongPhase)?
                    .controller();
                self.triggers.delay_sacrifice(
                    trigger.source.clone(),
                    controller,
                    crate::turn::Step::EndCombat,
                    vec![source],
                );
                return Ok(());
            }
            CombatTriggerKeyword::Mobilize(_) => return Err(EffectError::InconsistentProgram), // Created by the token-entry driver.
            CombatTriggerKeyword::Ingest => {
                let [cause] = trigger.causes.members() else {
                    return Err(EffectError::InconsistentProgram);
                };
                let crate::occurrence::TriggerRelated::CombatDamageToPlayer { player, .. } =
                    cause.related
                else {
                    return Err(EffectError::InconsistentProgram);
                };
                return self.exile_top_for_ingest(entry, instruction, player, append);
            }
            CombatTriggerKeyword::Poisonous(amount) => {
                let [cause] = trigger.causes.members() else {
                    return Err(EffectError::InconsistentProgram);
                };
                let crate::occurrence::TriggerRelated::CombatDamageToPlayer { player, .. } =
                    cause.related
                else {
                    return Err(EffectError::InconsistentProgram);
                };
                let controller = trigger
                    .source
                    .object
                    .controller()
                    .ok_or(EffectError::InconsistentProgram)?;
                if !self.window.participants().contains(&player) || !amount.is_positive() {
                    return Ok(());
                }
                let before = self.rules_snapshot(None)?;
                if self.player_counter_prevented(
                    &before.view().evaluated,
                    player,
                    CounterKind::Poison,
                )? {
                    return Ok(());
                }
                let value = self.apply_poison(
                    player,
                    vec![crate::occurrence::PoisonContribution {
                        origin: crate::occurrence::PoisonOrigin::Poisonous(entry),
                        source: crate::damage::DamageSourceId::Object(source),
                        controller,
                        amount: amount.clone(),
                    }],
                );
                return self.emit_from(
                    EventCause::Instruction { entry, instruction },
                    &before,
                    None,
                    append,
                    |_, _, _| Ok(vec![value]),
                );
            }
            CombatTriggerKeyword::Prowess => (BTreeSet::from([source]), 1.into(), 1.into()),
            CombatTriggerKeyword::Bushido(amount) => {
                (BTreeSet::from([source]), amount.clone(), amount.clone())
            }
            CombatTriggerKeyword::Frenzy(amount) => {
                (BTreeSet::from([source]), amount.clone(), 0.into())
            }
            CombatTriggerKeyword::Rampage(amount) => {
                let count = self
                    .combat
                    .iter()
                    .flat_map(|combat| combat.blockers())
                    .filter(|row| row.attackers().contains(&source))
                    .count();
                let bonus = amount.times(&RulesInteger::from_usize(count.saturating_sub(1)));
                (BTreeSet::from([source]), bonus.clone(), bonus)
            }
            CombatTriggerKeyword::Flanking => {
                let [cause] = trigger.causes.members() else {
                    return Err(EffectError::InconsistentProgram);
                };
                let crate::occurrence::TriggerRelated::Combatant(blocker) = cause.related else {
                    return Err(EffectError::InconsistentProgram);
                };
                (BTreeSet::from([blocker]), (-1).into(), (-1).into())
            }
            CombatTriggerKeyword::BattleCry => {
                let attackers = self
                    .combat
                    .iter()
                    .flat_map(|combat| combat.attackers())
                    .map(|row| row.combatant().object())
                    .filter(|&id| id != source)
                    .collect();
                (attackers, 1.into(), 0.into())
            }
        };
        affected.retain(|id| {
            self.window
                .cards()
                .get(*id)
                .is_some_and(|card| card.zone() == ObjectZone::Battlefield)
        });
        self.modify_combat_objects(
            entry,
            instruction,
            affected,
            PowerToughness { power, toughness },
            append,
        )
    }

    fn modify_combat_objects(
        &mut self,
        entry: StackEntryId,
        instruction: usize,
        affected: BTreeSet<ObjectId>,
        adjustment: crate::characteristics::PowerToughness,
        append: &mut RulesJournal,
    ) -> Result<(), EffectError> {
        use crate::characteristics::{ResolvedDuration, ResolvedPart, ResolvedParts};
        if affected.is_empty() || (adjustment.power == 0.into() && adjustment.toughness == 0.into())
        {
            return Ok(());
        }
        let before = self.rules_snapshot(None)?;
        let effect = self.characteristics.add_effect_for_objects(
            entry,
            affected,
            ResolvedParts(vec![ResolvedPart::ModifyPT(adjustment)]),
            ResolvedDuration::UntilEndOfTurn,
        )?;
        self.emit_from(
            EventCause::Instruction { entry, instruction },
            &before,
            None,
            append,
            |previous, current, _| {
                let appearances = characteristic_appearance_changes(
                    previous,
                    current,
                    effect.affected.iter().copied(),
                )?;
                Ok(vec![RulesOccurrence::ContinuousEffectCreated {
                    effect,
                    appearances,
                }])
            },
        )
    }
    fn soulbond_allows_combat(
        &self,
        creature: &ObjectCharacteristics,
        world: &EvaluatedCharacteristics,
    ) -> bool {
        use crate::characteristics::BlockingRestriction;
        if !creature
            .blocking_restrictions
            .contains(&BlockingRestriction::RequiresSoulbondPartner)
        {
            return true;
        }
        self.characteristics
            .paired_creatures
            .get(&creature.object)
            .and_then(|partner| world.get(*partner))
            .is_some_and(|partner| {
                partner.zone == ObjectZone::Battlefield
                    && partner.is_creature()
                    && partner.controller == creature.controller
                    && (partner.printed_text.triggered.iter().any(|trigger| {
                        partner.abilities.triggered.contains(&trigger.id)
                            && matches!(
                                trigger.condition.event,
                                crate::program::TriggerEvent::Keyword(
                                    crate::program::TriggerKeywordEvent::Soulbond { .. }
                                )
                            )
                    }) || partner.granted_triggers.values().flatten().any(|trigger| {
                        matches!(
                            trigger.condition.event,
                            crate::program::TriggerEvent::Keyword(
                                crate::program::TriggerKeywordEvent::Soulbond { .. }
                            )
                        )
                    }))
            })
    }
    fn eligible_attackers(&self, actor: SeatId) -> Result<Vec<ObjectId>, EffectError> {
        let evaluated = self.evaluated_characteristics()?;
        Ok(self
            .window
            .cards()
            .iter()
            .filter(|card| {
                card.zone() == ObjectZone::Battlefield
                    && card.controller() == Some(actor)
                    && card.tapped() == Some(false)
                    && evaluated.get(card.id()).is_some_and(|current| {
                        current.is_creature()
                            && !current.has_type(CardType::Battle)
                            && (!current.has_keyword(KeywordAbility::Defender)
                                || current.blocking_restrictions.contains(
                                    &crate::characteristics::BlockingRestriction::CanAttackAsThoughNoDefender,
                                ))
                            && self.blessing_allows_combat(current, actor)
                            && self.soulbond_allows_combat(current, &evaluated)
                            && !current.blocking_restrictions.contains(
                                &crate::characteristics::BlockingRestriction::CannotAttackOrBlock,
                            )
                            && !current.blocking_restrictions.contains(
                                &crate::characteristics::BlockingRestriction::CannotAttack,
                            )
                            && (card.has_continuous_control_since_turn_start() == Some(true)
                                || current.has_keyword(KeywordAbility::Haste))
                    })
            })
            .map(GameObject::id)
            .collect())
    }
    /// Player defending this attackable permanent, including a battle's protector.
    pub fn permanent_defender(&self, view: &ObjectCharacteristics) -> Option<SeatId> {
        if view.zone != ObjectZone::Battlefield {
            return None;
        }
        if view.has_type(CardType::Battle) {
            self.characteristics
                .battle_protectors
                .get(&view.object)
                .copied()
        } else if view.has_type(CardType::Planeswalker) {
            view.controller
        } else {
            None
        }
    }

    pub(super) fn attack_destinations(
        &self,
        controller: SeatId,
        current: &EvaluatedCharacteristics,
    ) -> Result<BTreeMap<TargetRef, SeatId>, EffectError> {
        let opponents = self.opponents_of(controller)?;
        let mut defenders: BTreeMap<_, _> = opponents
            .iter()
            .map(|&player| (TargetRef::Player(player), player))
            .collect();
        for permanent in current.objects.values() {
            if let Some(player) = self
                .permanent_defender(permanent)
                .filter(|player| opponents.contains(player))
            {
                defenders.insert(TargetRef::Object(permanent.object), player);
            }
        }
        Ok(defenders)
    }

    pub(super) fn attack_defender_frame(&self) -> Result<Option<AttackDefenderFrame>, EffectError> {
        let entry = self
            .window
            .resolving()
            .ok_or(EffectError::WrongResolutionFrame)?;
        let cursor = self
            .resolution_cursor()
            .ok_or(EffectError::WrongResolutionFrame)?;
        let Some(attacker) = self.legal_permanent_targets()?.into_iter().next() else {
            return Ok(None);
        };
        let Some(membership) = self
            .combat
            .as_ref()
            .and_then(|combat| combat.attacker(attacker))
        else {
            return Ok(None);
        };
        let defenders = self.attack_destinations(
            membership.combatant().controller(),
            &self.evaluated_characteristics()?,
        )?;
        Ok(Some(AttackDefenderFrame {
            revision: self.revision,
            entry: entry.id(),
            instruction: cursor.instruction,
            chooser: entry.controller(),
            attacker,
            defenders,
        }))
    }
    pub(super) fn resume_attack_defender(
        &self,
        expected: u64,
        frame: &AttackDefenderFrame,
        actor: SeatId,
        selected: Option<TargetRef>,
    ) -> Result<RuleProgress, EffectError> {
        let revision = self.next_revision(expected)?;
        if actor != frame.chooser {
            return Err(EffectError::WrongChooser);
        }
        let is_reselect = matches!(
            self.current_instruction()?,
            Instruction::MutateObject(spec)
                if matches!(
                    spec.op,
                    crate::toolbox::ObjectMutation::Combat(
                        crate::toolbox::ObjectCombatOp::ReselectAttackDefender
                    )
                )
        );
        if !is_reselect || self.attack_defender_frame()?.as_ref() != Some(frame) {
            return Err(EffectError::WrongResolutionFrame);
        }
        let mut next = self.clone();
        next.revision = revision;
        let mut append = RulesJournal::default();
        if let Some(to) = selected {
            let &player = frame
                .defenders
                .get(&to)
                .ok_or(CombatError::IllegalDefender)?;
            let before = next.rules_snapshot(None)?;
            let combat = next.combat.as_mut().ok_or(CombatError::IllegalAttacker)?;
            let member = combat
                .attacker(frame.attacker)
                .ok_or(CombatError::IllegalAttacker)?;
            let from = member
                .defending_planeswalker()
                .map_or(TargetRef::Player(member.defender()), TargetRef::Object);
            if from != to {
                let walker = match to {
                    TargetRef::Player(_) => None,
                    TargetRef::Object(id) => Some(id),
                    TargetRef::Stack(_) => return Err(CombatError::IllegalDefender.into()),
                };
                combat.reselect_defender(frame.attacker, player, walker)?;
                next.emit_from(
                    crate::occurrence::EventCause::Instruction {
                        entry: frame.entry,
                        instruction: frame.instruction,
                    },
                    &before,
                    None,
                    &mut append,
                    |_, _, _| {
                        Ok(vec![
                            crate::occurrence::RulesOccurrence::AttackDefenderChanged {
                                attacker: frame.attacker,
                                from,
                                to,
                            },
                        ])
                    },
                )?;
            }
        }
        next.advance_instruction()?;
        next.progress_rules(
            DamageJournal::default(),
            vec![],
            vec![],
            DrawJournal::default(),
            append,
        )
    }
    pub(crate) fn begin_declare_attackers(
        &self,
        expected: u64,
        turn: &TurnScheduler,
    ) -> Result<RuleProgress, EffectError> {
        let revision = self.next_revision(expected)?;
        self.require_combat_work(turn, TurnWork::DeclareAttackers)?;
        if self.pending_work.is_some() || self.combat.is_some() {
            return Err(EffectError::WrongPhase);
        }
        let player = self.window.active_player();
        let defenders = if self.window.active_participant().is_some() {
            self.combat_rules
                .opponents(self.window.participants(), player)?
        } else {
            vec![]
        };
        let eligible = self.eligible_attackers(player)?;
        let evaluated = self.evaluated_characteristics()?;
        let eligible_permanents = evaluated.objects.values().filter(|view| {
            self.permanent_defender(view)
                .is_some_and(|seat| defenders.contains(&seat))
        });
        let planeswalkers = eligible_permanents
            .clone()
            .filter(|view| view.has_type(CardType::Planeswalker))
            .map(|view| view.object)
            .collect();
        let battles = eligible_permanents
            .filter(|view| view.has_type(CardType::Battle))
            .map(|view| view.object)
            .collect();
        let mut frame = AttackersDecision::new(revision, player, eligible, defenders)
            .with_planeswalkers(planeswalkers)
            .with_battles(battles);
        if self.window.active_participant().is_some() {
            frame.observe_banding(&evaluated, &self.name_rules);
            self.apply_attack_requirements(&mut frame, &evaluated)?;
        }
        let mut next = self.clone();
        next.revision = revision;
        if frame.eligible_attackers().is_empty() {
            let before = next.rules_snapshot(Some(turn))?;
            next.combat = Some(CombatState::new(vec![])?);
            let mut append = RulesJournal::default();
            next.emit_attackers_declaration(
                &before,
                turn,
                player,
                DeclarationOrigin::ForcedEmpty,
                vec![],
                &mut append,
            )?;
            return Ok(RuleProgress::new(
                next,
                RuleResult::Complete {
                    checks: CheckJournal::default(),
                    journal: DamageJournal::default(),
                    assignments: vec![],
                    permanent_entry: None,
                    completion: RuleCompletion::TurnWork(WorkCompleted::DeclareAttackers),
                    declarations: vec![CombatDeclarationRecord::Attackers {
                        player,
                        origin: DeclarationOrigin::ForcedEmpty,
                        attackers: vec![],
                        taps: vec![],
                    }],
                    draws: DrawJournal::default(),
                },
                append,
            ));
        }
        let frame = CombatDecision::Attackers(frame);
        next.pending_work = Some(PendingWork::Combat(CombatCursor {
            attack_payment: None,
            frame: frame.clone(),
            remaining_actors: vec![],
            assignments: vec![],
        }));
        Ok(RuleProgress::new(
            next,
            RuleResult::Pending {
                checks: CheckJournal::default(),
                journal: DamageJournal::default(),
                assignments: vec![],
                frame: EffectDecision::Combat(frame),
                declarations: vec![],
                draws: DrawJournal::default(),
            },
            RulesJournal::default(),
        ))
    }

    pub(super) fn combat_declaration_departure_admitted(
        &self,
        issued: Option<&EffectDecision>,
    ) -> bool {
        matches!((&self.pending_work, issued),
            (Some(PendingWork::Combat(cursor)), Some(EffectDecision::Combat(frame)))
            if cursor.attack_payment.is_none() && cursor.frame == *frame
                && matches!(frame, CombatDecision::Attackers(_) | CombatDecision::Blockers(_)))
    }

    /// Reissue the exact pending declaration and its unvisited suffix against the living table.
    pub(super) fn progress_combat_declaration_after_departure(
        mut self,
        turn: &TurnScheduler,
    ) -> Result<RuleProgress, EffectError> {
        let Some(PendingWork::Combat(cursor)) = self.pending_work.take() else {
            return Err(EffectError::WrongPhase);
        };
        match cursor.frame {
            CombatDecision::Attackers(_) => self.begin_declare_attackers(self.revision, turn),
            CombatDecision::Blockers(frame) => {
                self.require_combat_work(turn, TurnWork::DeclareBlockers)?;
                let remaining = std::iter::once(frame.defending_player())
                    .chain(cursor.remaining_actors)
                    .filter(|seat| self.window.participants().contains(seat))
                    .collect();
                self.advance_blocker_actors(remaining, vec![], turn, RulesJournal::default())
            }
            CombatDecision::DamageAssignments(_) => Err(EffectError::WrongPhase),
        }
    }
    pub(crate) fn begin_declare_blockers(
        &self,
        expected: u64,
        turn: &TurnScheduler,
    ) -> Result<RuleProgress, EffectError> {
        let revision = self.next_revision(expected)?;
        self.require_combat_work(turn, TurnWork::DeclareBlockers)?;
        if self.pending_work.is_some() {
            return Err(EffectError::WrongPhase);
        }
        let combat = self.combat.as_ref().ok_or(CombatError::InvalidContext)?;
        if !combat.had_attackers() {
            return Err(CombatError::InvalidContext.into());
        }
        let attacked: BTreeSet<_> = combat.attackers().map(|a| a.defender()).collect();
        let remaining = if let Some(active) = self.window.active_participant() {
            self.combat_rules
                .opponents(self.window.participants(), active)?
                .into_iter()
                .filter(|player| attacked.contains(player))
                .collect()
        } else {
            vec![]
        };
        let mut next = self.clone();
        next.revision = revision;
        next.combat
            .as_mut()
            .ok_or(CombatError::InvalidContext)?
            .begin_blockers();
        next.advance_blocker_actors(remaining, vec![], turn, RulesJournal::default())
    }
    /// Process only actual attacked actors. Unattacked opponents have no player
    /// submission; a singleton empty block domain is explicitly kernel-completed.
    fn advance_blocker_actors(
        mut self,
        mut remaining: Vec<SeatId>,
        mut records: Vec<CombatDeclarationRecord>,
        turn: &TurnScheduler,
        mut append: RulesJournal,
    ) -> Result<RuleProgress, EffectError> {
        self.pending_work = None;
        while !remaining.is_empty() {
            let player = remaining.remove(0);
            let domain = self.blocking_domain(player)?;
            if domain.attackers().is_empty() {
                continue;
            }
            if domain.eligible_blockers().is_empty() {
                self.declare_blocks_at_cut(
                    player,
                    DeclarationOrigin::ForcedEmpty,
                    vec![],
                    turn,
                    &mut append,
                )?;
                records.push(CombatDeclarationRecord::Blockers {
                    player,
                    origin: DeclarationOrigin::ForcedEmpty,
                    blockers: vec![],
                });
                continue;
            }
            let frame = CombatDecision::Blockers(domain);
            self.pending_work = Some(PendingWork::Combat(CombatCursor {
                attack_payment: None,
                frame: frame.clone(),
                remaining_actors: remaining,
                assignments: vec![],
            }));
            return Ok(RuleProgress::new(
                self,
                RuleResult::Pending {
                    checks: CheckJournal::default(),
                    journal: DamageJournal::default(),
                    assignments: vec![],
                    frame: EffectDecision::Combat(frame),
                    declarations: records,
                    draws: DrawJournal::default(),
                },
                append,
            ));
        }
        Ok(RuleProgress::new(
            self,
            RuleResult::Complete {
                checks: CheckJournal::default(),
                journal: DamageJournal::default(),
                assignments: vec![],
                permanent_entry: None,
                completion: RuleCompletion::TurnWork(WorkCompleted::DeclareBlockers),
                declarations: records,
                draws: DrawJournal::default(),
            },
            append,
        ))
    }

    /// Validate the actual post-membership view before publishing the one declaration.
    fn emit_attackers_declaration(
        &mut self,
        before: &super::occurrence_flow::RulesSnapshot,
        turn: &TurnScheduler,
        actor: SeatId,
        origin: DeclarationOrigin,
        selected: Vec<crate::combat::AttackDeclaration>,
        append: &mut RulesJournal,
    ) -> Result<(), EffectError> {
        self.emit_from(
            EventCause::AttackersDeclared {
                position: turn.position(),
                actor,
            },
            before,
            Some(turn),
            append,
            |previous, current, _| {
                let calendar = current.turn.ok_or(CombatError::InvalidContext)?;
                let combat = current.combat.ok_or(CombatError::InvalidContext)?;
                let membership: Vec<_> = combat.attackers().cloned().collect();
                if calendar.stage() != TurnStage::Work(TurnWork::DeclareAttackers)
                    || current.window.active_player() != actor
                    || previous.combat.is_some()
                    || membership.len() != selected.len()
                    || membership
                        .iter()
                        .zip(&selected)
                        .any(|(member, declaration)| {
                            member.combatant().object() != declaration.attacker
                                || member.defender() != declaration.defender
                                || member.defending_planeswalker()
                                    != declaration.defending_planeswalker
                        })
                {
                    return Err(CombatError::InvalidContext.into());
                }
                Ok(vec![RulesOccurrence::AttackersDeclared {
                    origin,
                    selected,
                    membership,
                }])
            },
        )
    }

    /// The whole defender write has one pre/final status pair per attacked creature.
    fn declare_blocks_at_cut(
        &mut self,
        actor: SeatId,
        origin: DeclarationOrigin,
        participants: Vec<(Combatant, ObjectId)>,
        turn: &TurnScheduler,
        append: &mut RulesJournal,
    ) -> Result<(), EffectError> {
        let before = self.rules_snapshot(Some(turn))?;
        let previous = before.view();
        let calendar = previous.turn.ok_or(CombatError::InvalidContext)?;
        if calendar.stage() != TurnStage::Work(TurnWork::DeclareBlockers) {
            return Err(CombatError::InvalidContext.into());
        }
        let statuses: Vec<_> = previous
            .combat
            .ok_or(CombatError::InvalidContext)?
            .attackers()
            .filter(|attacker| attacker.defender() == actor)
            .map(|attacker| (attacker.combatant().object(), attacker.block_status()))
            .collect();
        let combat = self.combat.as_mut().ok_or(CombatError::InvalidContext)?;
        let selected = combat.declare_blocks(actor, participants)?;
        self.emit_from(
            EventCause::BlockersDeclared {
                position: calendar.position(),
                actor,
            },
            &before,
            Some(turn),
            append,
            |_, current, _| {
                let combat = current.combat.ok_or(CombatError::InvalidContext)?;
                let statuses = statuses
                    .into_iter()
                    .map(|(object, old)| {
                        let new = combat
                            .attacker(object)
                            .ok_or(CombatError::InvalidContext)?
                            .block_status();
                        Ok((object, old, new))
                    })
                    .collect::<Result<Vec<_>, CombatError>>()?;
                let membership = combat.blockers().cloned().collect();
                Ok(vec![RulesOccurrence::BlockersDeclared {
                    origin,
                    selected,
                    membership,
                    statuses,
                }])
            },
        )
    }
    pub(crate) fn has_attack_mana_child(&self) -> bool {
        match &self.pending_work {
            Some(PendingWork::ManaColor(work)) => {
                matches!(work.parent, ManaParent::CombatPayment(_))
            }
            Some(PendingWork::ManaSacrifice(work)) => {
                matches!(work.parent, ManaParent::CombatPayment(_))
            }
            Some(PendingWork::Damage(work)) => matches!(
                work.parent,
                DamageParent::Mana(ManaParent::CombatPayment(_))
            ),
            _ => false,
        }
    }

    pub(crate) fn is_paying_attack_cost(&self) -> bool {
        matches!(&self.pending_work, Some(PendingWork::Combat(cursor)) if cursor.attack_payment.is_some())
    }

    fn await_attack_payment(self, frame: CombatDecision, append: RulesJournal) -> RuleProgress {
        RuleProgress::new(
            self,
            RuleResult::Pending {
                frame: EffectDecision::Combat(frame),
                checks: CheckJournal::default(),
                declarations: Vec::new(),
                assignments: Vec::new(),
                draws: DrawJournal::default(),
                journal: DamageJournal::default(),
            },
            append,
        )
    }

    fn resume_attack_payment(
        &self,
        revision: u64,
        work: &crate::combat::AttackPaymentWork,
        decision: &CombatDecision,
        actor: SeatId,
        reply: DecisionReply,
        turn: &TurnScheduler,
    ) -> Result<RuleProgress, EffectError> {
        self.require_combat_work(turn, TurnWork::DeclareAttackers)?;
        let CombatDecision::Attackers(frame) = decision else {
            return Err(CombatError::WrongFrame.into());
        };
        let cost = frame.payment().ok_or(CombatError::WrongFrame)?;
        if matches!(
            reply,
            DecisionReply::RedeclareAttackers | DecisionReply::RedeclareKeepingMana
        ) {
            return self.reverse_attack_declaration(
                work,
                actor,
                turn,
                revision,
                reply == DecisionReply::RedeclareKeepingMana,
            );
        }
        let DecisionReply::PayAttackCost { life_symbols, mana } = reply else {
            return Err(EffectError::WrongDecisionKind);
        };
        if life_symbols < 0.into() || life_symbols > cost.white_phyrexian {
            return Err(EffectError::InvalidCastResource);
        }
        let mut price = ManaCost::generic(cost.generic.clone());
        let white = cost.white_phyrexian.minus(&life_symbols);
        if white.is_positive() {
            price.required.insert(ManaType::White, white);
        }
        let spend = prepare_payment(&price, Some(&mana), |kind| {
            self.unrestricted_mana(actor, kind)
        })
        .map_err(EffectError::InvalidManaPayment)?;
        let mut next = self.clone();
        next.revision = revision;
        next.pending_work = None;
        let mut append = RulesJournal::default();
        let cause = EventCause::AttackCostPaid {
            position: turn.position(),
            player: actor,
        };
        next.pay_life_at_cut(
            actor,
            life_symbols.times(&2.into()),
            cause.clone(),
            Some(turn),
            &mut append,
        )?;
        let before = next.rules_snapshot(Some(turn))?;
        next.spend_mana_lots(actor, &spend, &[], Some(&mana.from_lots))?;
        next.debit_mana(actor, &spend);
        next.emit_from(cause, &before, Some(turn), &mut append, |_, _, _| {
            Ok(vec![RulesOccurrence::AttackCostPaid {
                player: actor,
                cost: cost.clone(),
                white_symbols_paid_with_life: life_symbols,
                mana: spend,
            }])
        })?;
        next.finish_paid_attack(
            actor,
            work.selected.clone(),
            work.bands.clone(),
            work.taps.clone(),
            turn,
            append,
        )
    }

    fn reverse_attack_declaration(
        &self,
        work: &crate::combat::AttackPaymentWork,
        actor: SeatId,
        turn: &TurnScheduler,
        revision: u64,
        keep_mana: bool,
    ) -> Result<RuleProgress, EffectError> {
        // The transaction checks hidden information and funding dependencies before selecting this endpoint.
        let kept_mana_abilities = keep_mana;
        let mut next = if kept_mana_abilities {
            let mut state = self.clone();
            state.window = state.window.reverse_declaration_taps(actor, &work.taps)?;
            state
                .triggers
                .discard_undone_attack_taps(&work.tap_triggers);
            state
        } else {
            let mut restored = work.before.as_ref().clone();
            restored.retain_attack_allocation_cut(&self.attack_allocation_cut());
            restored
        };
        next.pending_work = None;
        next.revision = revision;
        let (next, result, later) = next.begin_declare_attackers(revision, turn)?.into_parts();
        let mut append = RulesJournal {
            records: vec![crate::occurrence::RulesRecord::AttackDeclarationReversed {
                player: actor,
                attackers: work.selected.iter().map(|row| row.attacker).collect(),
                kept_mana_abilities,
                reversed_mana: BTreeSet::new(),
                retracted_tap_triggers: work.tap_triggers.clone(),
            }],
        };
        append.append(later);
        Ok(RuleProgress::new(next, result, append))
    }

    fn finish_paid_attack(
        mut self,
        actor: SeatId,
        mut selected: Vec<crate::combat::AttackDeclaration>,
        bands: Vec<Vec<ObjectId>>,
        taps: Vec<AttackTap>,
        turn: &TurnScheduler,
        mut append: RulesJournal,
    ) -> Result<RuleProgress, EffectError> {
        let current = self.evaluated_characteristics()?;
        selected.retain(|a| {
            current.get(a.attacker).is_some_and(|object| {
                object.zone == ObjectZone::Battlefield
                    && object.controller == Some(actor)
                    && object.is_creature()
            })
        });
        let participants = selected
            .iter()
            .map(|a| {
                self.window
                    .cards()
                    .get(a.attacker)
                    .and_then(Combatant::from_object)
                    .map(|combatant| (combatant, a.defender))
                    .ok_or(CombatError::IllegalAttacker)
            })
            .collect::<Result<Vec<_>, _>>()?;
        let before = self.rules_snapshot(Some(turn))?;
        let mut combat = CombatState::new(participants)?;
        combat.set_planeswalker_targets(&selected)?;
        combat.set_attacking_bands(bands);
        self.combat = Some(combat);
        self.emit_attackers_declaration(
            &before,
            turn,
            actor,
            DeclarationOrigin::PlayerReply,
            selected.clone(),
            &mut append,
        )?;
        Ok(RuleProgress::new(
            self,
            RuleResult::Complete {
                checks: CheckJournal::default(),
                journal: DamageJournal::default(),
                assignments: Vec::new(),
                permanent_entry: None,
                completion: RuleCompletion::TurnWork(WorkCompleted::DeclareAttackers),
                declarations: vec![CombatDeclarationRecord::Attackers {
                    player: actor,
                    origin: DeclarationOrigin::PlayerReply,
                    attackers: selected,
                    taps,
                }],
                draws: DrawJournal::default(),
            },
            append,
        ))
    }

    pub(crate) fn resume_combat(
        &self,
        expected: u64,
        frame: &CombatDecision,
        actor: SeatId,
        choice: DecisionReply,
        turn: &TurnScheduler,
    ) -> Result<RuleProgress, EffectError> {
        let revision = self.next_revision(expected)?;
        let Some(PendingWork::Combat(cursor)) = self.pending_work.as_ref() else {
            return Err(CombatError::WrongFrame.into());
        };
        if let Some(work) = &cursor.attack_payment {
            if self.pending_resolution_decision() != Some(EffectDecision::Combat(frame.clone())) {
                return Err(CombatError::WrongFrame.into());
            }
            if frame.chooser() != actor {
                return Err(EffectError::WrongChooser);
            }
            return self.resume_attack_payment(revision, work, frame, actor, choice, turn);
        }
        if &cursor.frame != frame || frame.revision() != self.revision {
            return Err(CombatError::WrongFrame.into());
        }
        if frame.chooser() != actor {
            return Err(EffectError::WrongChooser);
        }
        let (choice, bands) = match choice {
            DecisionReply::DeclareBandedAttackers { attackers, bands } => {
                (DecisionReply::DeclareAttackers(attackers), bands)
            }
            other => (other, vec![]),
        };
        match (frame, choice) {
            (CombatDecision::Attackers(bound), DecisionReply::DeclareAttackers(mut selected)) => {
                bound.validate_bands(&selected, &bands)?;
                self.require_combat_work(turn, TurnWork::DeclareAttackers)?;
                if actor != self.window.active_player() || self.combat.is_some() {
                    return Err(CombatError::InvalidContext.into());
                }
                let eligible = self.eligible_attackers(actor)?;
                let defenders = self
                    .combat_rules
                    .opponents(self.window.participants(), actor)?;
                let mut seen = BTreeSet::new();
                for declaration in &selected {
                    if !seen.insert(declaration.attacker) {
                        return Err(CombatError::DuplicateAttacker.into());
                    }
                    if !bound.eligible_attackers().contains(&declaration.attacker)
                        || !eligible.contains(&declaration.attacker)
                    {
                        return Err(CombatError::IllegalAttacker.into());
                    }
                    if !bound.defenders().contains(&declaration.defender)
                        || !defenders.contains(&declaration.defender)
                    {
                        return Err(CombatError::IllegalDefender.into());
                    }
                    let target = declaration
                        .defending_planeswalker
                        .map_or(TargetRef::Player(declaration.defender), TargetRef::Object);
                    if bound
                        .forbidden_defenders()
                        .get(&declaration.attacker)
                        .is_some_and(|set| set.contains(&target))
                    {
                        return Err(CombatError::IllegalDefender.into());
                    }
                    if let Some(id) = declaration.defending_planeswalker
                        && (!bound.planeswalkers().contains(&id) && !bound.battles().contains(&id)
                            || !self.current_characteristics(id)?.is_some_and(|view| {
                                self.permanent_defender(&view) == Some(declaration.defender)
                            }))
                    {
                        return Err(CombatError::IllegalDefender.into());
                    }
                }
                bound.validate_defender_caps(&selected)?;
                if selected.len() == 1
                    && self
                        .current_characteristics(selected[0].attacker)?
                        .is_some_and(|view| {
                            view.blocking_restrictions.contains(
                                &crate::characteristics::BlockingRestriction::CannotAttackAlone,
                            )
                        })
                {
                    return Err(CombatError::IllegalAttacker.into());
                }
                if bound.single_attacker()
                    && (selected.len() > 1
                        || (selected.is_empty() && !bound.required_attacks().is_empty()))
                {
                    return Err(CombatError::IllegalAttacker.into());
                }
                for (required, legal_targets) in bound.required_attacks() {
                    if bound.single_attacker() && !seen.contains(required) {
                        continue;
                    }
                    let attack = selected
                        .iter()
                        .find(|a| a.attacker == *required)
                        .ok_or(CombatError::IllegalAttacker)?;
                    let target = attack
                        .defending_planeswalker
                        .map_or(TargetRef::Player(attack.defender), TargetRef::Object);
                    if !legal_targets.contains(&target) {
                        return Err(CombatError::IllegalDefender.into());
                    }
                }
                selected.sort_unstable_by_key(|a| a.attacker);
                let before_taps = self.rules_snapshot(Some(turn))?;
                let evaluated = &before_taps.view().evaluated;
                let objects: Vec<_> = selected
                    .iter()
                    .filter(|declaration| {
                        !evaluated
                            .get(declaration.attacker)
                            .is_some_and(|current| current.has_keyword(KeywordAbility::Vigilance))
                    })
                    .map(|declaration| declaration.attacker)
                    .collect();
                let (window, changes) =
                    self.window
                        .record_declaration_taps(self.window.revision(), actor, &objects)?;
                let mut next = self.clone();
                next.revision = revision;
                next.window = window;
                next.pending_work = None;
                let mut append = RulesJournal::default();
                if before_taps.view().combat.is_some() || next.combat.is_some() {
                    return Err(CombatError::InvalidContext.into());
                }
                if !changes.is_empty() {
                    next.emit_from(
                        EventCause::AttackTaps {
                            position: turn.position(),
                            actor,
                        },
                        &before_taps,
                        Some(turn),
                        &mut append,
                        |_, _, _| {
                            Ok(super::occurrence_flow::status_occurrences(
                                changes.clone(),
                                StatusReason::AttackDeclaration,
                            ))
                        },
                    )?;
                }
                let taps = changes
                    .into_iter()
                    .map(|change| AttackTap {
                        before: change.before,
                        after: change.after,
                    })
                    .collect::<Vec<_>>();
                let evaluated = next.evaluated_characteristics()?;
                let mut cost = crate::combat::AttackCost::default();
                for attack in &selected {
                    let target = attack
                        .defending_planeswalker
                        .map_or(TargetRef::Player(attack.defender), TargetRef::Object);
                    cost.add(&next.defender_attack_cost(target, &evaluated)?);
                }
                if cost != crate::combat::AttackCost::default() {
                    let mut offered = bound.at_revision(revision);
                    offered.payment = Some(cost);
                    let frame = CombatDecision::Attackers(offered);
                    next.pending_work = Some(PendingWork::Combat(CombatCursor {
                        attack_payment: Some(Box::new(crate::combat::AttackPaymentWork {
                            selected,
                            bands,
                            taps,
                            before: Arc::new(self.clone()),
                            tap_triggers: next
                                .triggers
                                .waiting()
                                .keys()
                                .filter(|id| !self.triggers.waiting().contains_key(id))
                                .copied()
                                .collect(),
                        })),
                        frame: frame.clone(),
                        remaining_actors: Vec::new(),
                        assignments: Vec::new(),
                    }));
                    return Ok(next.await_attack_payment(frame, append));
                }
                next.finish_paid_attack(actor, selected, bands, taps, turn, append)
            }
            (CombatDecision::Blockers(bound), DecisionReply::DeclareBlockers(mut selected)) => {
                self.require_combat_work(turn, TurnWork::DeclareBlockers)?;
                let defender = bound.defending_player();
                if self.blocking_domain(defender)? != *bound {
                    return Err(CombatError::WrongFrame.into());
                }
                bound.validate(&selected)?;
                let distinct_blockers: BTreeSet<ObjectId> =
                    selected.iter().map(|b| b.blocker).collect();
                if distinct_blockers.len() == 1
                    && let Some(&sole_blocker) = distinct_blockers.iter().next()
                    && self
                        .current_characteristics(sole_blocker)?
                        .is_some_and(|view| {
                            view.blocking_restrictions.contains(
                                &crate::characteristics::BlockingRestriction::CannotBlockAlone,
                            )
                        })
                {
                    return Err(CombatError::IllegalBlocker.into());
                }
                selected.sort_unstable_by_key(|b| b.blocker);
                let participants = selected
                    .iter()
                    .map(|b| {
                        let participant = self
                            .window
                            .cards()
                            .get(b.blocker)
                            .and_then(Combatant::from_object)
                            .ok_or(CombatError::IllegalBlocker)?;
                        Ok((participant, b.attacker))
                    })
                    .collect::<Result<Vec<_>, CombatError>>()?;
                let mut next = self.clone();
                next.revision = revision;
                let mut append = RulesJournal::default();
                next.declare_blocks_at_cut(
                    defender,
                    DeclarationOrigin::PlayerReply,
                    participants,
                    turn,
                    &mut append,
                )?;
                next.advance_blocker_actors(
                    cursor.remaining_actors.clone(),
                    vec![CombatDeclarationRecord::Blockers {
                        player: defender,
                        origin: DeclarationOrigin::PlayerReply,
                        blockers: selected,
                    }],
                    turn,
                    append,
                )
            }
            _ => Err(EffectError::WrongDecisionKind),
        }
    }
    /// Root-only actual phase exit. This is not an action or damage-work bypass.
    pub(crate) fn finish_combat(
        &self,
        expected: u64,
    ) -> Result<RulesTransition<Self, ()>, EffectError> {
        let revision = self.next_revision(expected)?;
        if self.pending_work.is_some()
            || self.combat.is_none()
            || self.window.phase() != WindowPhase::StepEnded
        {
            return Err(EffectError::WrongPhase);
        }
        if !self.state_based_losses.is_empty() {
            return Err(EffectError::PlayerLossPending);
        }
        Ok(RulesTransition::new(
            Self {
                revision,
                combat: None,
                ..self.clone()
            },
            (),
            RulesJournal::default(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        game::{GameCommand, GameContext, GameId, GameInput, GameState, RuleFrame},
        turn::{Step, TurnStart},
    };

    const A: SeatId = SeatId(17);
    const B: SeatId = SeatId(4);

    fn pending_game(blockers: bool) -> GameState {
        let cards: Vec<SeedCard> = [(1, A, false), (2, B, false), (3, A, true)]
            .into_iter()
            .map(|(id, owner, library)| SeedCard {
                card: CardId(id),
                owner,
                destination: if library {
                    ObjectDestination::LibraryBottom
                } else {
                    ObjectDestination::Battlefield { controller: owner }
                },
                program: Program::creature(
                    None,
                    2.into(),
                    4.into(),
                    std::collections::BTreeSet::new(),
                ),
            })
            .collect();
        let mut game = GameState::initialize_scheduled(
            GameId(508),
            GameContext::new(
                "combat-private".into(),
                "qualification-synthetic".into(),
                "authored".into(),
                "ordinary".into(),
                "explicit-ffa".into(),
                crate::test_support::creature_types::synthetic_vocabulary().id(),
                spellwake_contracts::names::NameRules::literal_names().id(),
            )
            .unwrap(),
            {
                // Fixture premise: seeded BF timestamps follow this authored card sequence.
                let initial_battlefield_order = cards
                    .iter()
                    .filter(|card| {
                        matches!(
                            card.destination,
                            crate::object_identity::ObjectDestination::Battlefield { .. }
                        )
                    })
                    .map(|card| card.card)
                    .collect();
                GameSeed::with_uniform_life(
                    vec![A, B],
                    A,
                    cards,
                    initial_battlefield_order,
                    vec![],
                    20.into(),
                    CombatRules::UnrestrictedFreeForAll,
                    crate::test_support::creature_types::synthetic_vocabulary(),
                    std::sync::Arc::new(spellwake_contracts::names::NameRules::literal_names()),
                )
            },
            TurnStart {
                number: 1,
                step: Step::Untap,
            },
        )
        .unwrap()
        .into_state();
        loop {
            if let Some(decision) = game.pending_decision() {
                if !blockers
                    || matches!(
                        decision.frame(),
                        RuleFrame::Combat(CombatDecision::Blockers(_))
                    )
                {
                    return game;
                }
                let attacker = game
                    .execution()
                    .window()
                    .cards()
                    .current_identity(CardId(1))
                    .unwrap();
                game = game
                    .advance(GameInput {
                        basis: game.basis().clone(),
                        actor: A,
                        command: GameCommand::Reply {
                            decision: decision.id(),
                            choice: DecisionReply::DeclareAttackers(vec![
                                crate::combat::AttackDeclaration {
                                    attacker,
                                    defender: B,
                                    defending_planeswalker: None,
                                },
                            ]),
                        },
                    })
                    .unwrap()
                    .into_state();
            } else {
                let WindowPhase::Priority(crate::priority::PriorityPhase::Ready { holder }) =
                    game.execution().window().phase()
                else {
                    panic!("actual root must reach a decision or priority");
                };
                game = game
                    .advance(GameInput {
                        basis: game.basis().clone(),
                        actor: holder,
                        command: GameCommand::Pass,
                    })
                    .unwrap()
                    .into_state();
            }
        }
    }
    fn frame(game: &GameState) -> CombatDecision {
        let RuleFrame::Combat(frame) = game.pending_decision().unwrap().frame() else {
            panic!("combat frame");
        };
        frame.clone()
    }
    fn empty_choice(frame: &CombatDecision) -> DecisionReply {
        match frame {
            CombatDecision::Attackers(_) => DecisionReply::DeclareAttackers(vec![]),
            CombatDecision::Blockers(_) => DecisionReply::DeclareBlockers(vec![]),
            CombatDecision::DamageAssignments(_) => panic!("declaration fixture expected"),
        }
    }
    fn changed_frame(frame: &CombatDecision, revision: u64) -> CombatDecision {
        match frame {
            CombatDecision::Attackers(f) => CombatDecision::Attackers(AttackersDecision::new(
                revision,
                f.chooser(),
                vec![],
                f.defenders().to_vec(),
            )),
            CombatDecision::DamageAssignments(_) => panic!("declaration fixture expected"),
            CombatDecision::Blockers(f) => CombatDecision::Blockers(BlockersDecision::new(
                revision,
                f.chooser(),
                f.attackers().to_vec(),
                BTreeMap::new(),
                BTreeMap::new(),
            )),
        }
    }

    #[test]
    fn same_revision_altered_domains_and_stale_frames_preserve_pending_work() {
        for blockers in [false, true] {
            let game = pending_game(blockers);
            let state = game.execution();
            let frame = frame(&game);
            let saved = state.clone();
            for revision in [frame.revision(), frame.revision() - 1] {
                let wrong = changed_frame(&frame, revision);
                assert_eq!(
                    state.resume_combat(
                        state.revision(),
                        &wrong,
                        frame.chooser(),
                        empty_choice(&frame),
                        game.turn().unwrap()
                    ),
                    Err(EffectError::Combat(CombatError::WrongFrame))
                );
                assert_eq!(state, &saved);
            }
            assert!(
                state
                    .resume_combat(
                        state.revision(),
                        &frame,
                        frame.chooser(),
                        empty_choice(&frame),
                        game.turn().unwrap()
                    )
                    .is_ok()
            );
            assert_eq!(state, &saved);
        }
    }

    #[test]
    fn exact_cursor_frame_also_requires_the_current_executor_revision() {
        for blockers in [false, true] {
            let game = pending_game(blockers);
            let frame = frame(&game);
            let mut state = game.execution().clone();
            state.revision += 1;
            let saved = state.clone();
            assert_eq!(
                state.resume_combat(
                    state.revision(),
                    &frame,
                    frame.chooser(),
                    empty_choice(&frame),
                    game.turn().unwrap()
                ),
                Err(EffectError::Combat(CombatError::WrongFrame))
            );
            assert_eq!(state, saved);
        }
    }

    #[test]
    fn a_frame_without_its_owned_cursor_cannot_resume() {
        for blockers in [false, true] {
            let game = pending_game(blockers);
            let frame = frame(&game);
            let mut state = game.execution().clone();
            state.pending_work = None;
            let saved = state.clone();
            assert_eq!(
                state.resume_combat(
                    state.revision(),
                    &frame,
                    frame.chooser(),
                    empty_choice(&frame),
                    game.turn().unwrap()
                ),
                Err(EffectError::Combat(CombatError::WrongFrame))
            );
            assert_eq!(state, saved);
        }
    }

    #[test]
    fn correct_frames_still_require_the_actual_chooser_and_scheduled_work() {
        for blockers in [false, true] {
            let game = pending_game(blockers);
            let state = game.execution();
            let frame = frame(&game);
            let saved = state.clone();
            let other = if frame.chooser() == A { B } else { A };
            assert_eq!(
                state.resume_combat(
                    state.revision(),
                    &frame,
                    other,
                    empty_choice(&frame),
                    game.turn().unwrap()
                ),
                Err(EffectError::WrongChooser)
            );
            let wrong_turn = TurnScheduler::new(
                TurnStart {
                    number: 1,
                    step: Step::PrecombatMain,
                },
                false,
            )
            .unwrap();
            assert_eq!(
                state.resume_combat(
                    state.revision(),
                    &frame,
                    frame.chooser(),
                    empty_choice(&frame),
                    &wrong_turn
                ),
                Err(EffectError::WrongPhase)
            );
            assert_eq!(state, &saved);
        }
    }

    #[test]
    fn combat_pending_rejects_idle_and_resolution_operations_without_losing_cursor() {
        for blockers in [false, true] {
            let game = pending_game(blockers);
            let state = game.execution();
            let saved = state.clone();
            assert_eq!(
                state.ensure_resolution_or_idle(),
                Err(EffectError::WrongPhase)
            );
            assert_eq!(
                state.pass(state.revision(), A),
                Err(EffectError::WrongPhase)
            );
            assert_eq!(
                state.settle_checks(state.revision()),
                Err(EffectError::WrongPhase)
            );
            assert_eq!(
                state.begin_resolution(state.revision()),
                Err(EffectError::WrongPhase)
            );
            assert_eq!(
                state.begin_mana_activation(
                    state.revision(),
                    A,
                    state.window().cards().current_identity(CardId(1)).unwrap(),
                    crate::mana::ManaAbilityRef::Printed(ManaAbilityId(1))
                ),
                Err(EffectError::WrongPhase)
            );
            assert_eq!(state, &saved);
            let frame = frame(&game);
            assert!(
                state
                    .resume_combat(
                        state.revision(),
                        &frame,
                        frame.chooser(),
                        empty_choice(&frame),
                        game.turn().unwrap()
                    )
                    .is_ok()
            );
            assert_eq!(state, &saved);
        }
    }
}
