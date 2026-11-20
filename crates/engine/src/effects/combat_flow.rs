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

#[cfg(test)]
mod occurrence_tests;

impl EffectWindow {
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
                            && (card.has_continuous_control_since_turn_start() == Some(true)
                                || current.has_keyword(KeywordAbility::Haste))
                    })
            })
            .map(GameObject::id)
            .collect())
    }
    fn eligible_blockers(&self, actor: SeatId) -> Result<Vec<ObjectId>, EffectError> {
        let evaluated = self.evaluated_characteristics()?;
        Ok(self
            .window
            .cards()
            .iter()
            .filter(|card| {
                card.zone() == ObjectZone::Battlefield
                    && card.controller() == Some(actor)
                    && card.tapped() == Some(false)
                    && evaluated
                        .get(card.id())
                        .is_some_and(ObjectCharacteristics::is_creature)
            })
            .map(GameObject::id)
            .collect())
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
        let player = self.window.turn_owner();
        let defenders = self
            .combat_rules
            .opponents(self.window.participants(), player)?;
        let eligible = self.eligible_attackers(player)?;
        let mut next = self.clone();
        next.revision = revision;
        if eligible.is_empty() {
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
        let frame = CombatDecision::Attackers(AttackersDecision::new(
            revision, player, eligible, defenders,
        ));
        next.pending_work = Some(PendingWork::Combat(CombatCursor {
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
        let remaining = self
            .window
            .apnap_players()?
            .filter(|player| attacked.contains(player))
            .collect();
        let mut next = self.clone();
        next.revision = revision;
        next.advance_blocker_actors(remaining, vec![], turn, RulesJournal::default())
    }
    /// Process only actual attacked actors. Unattacked opponents have no player
    /// submission; a singleton empty block domain is explicitly kernel-completed.
    pub(super) fn advance_blocker_actors(
        mut self,
        mut remaining: Vec<SeatId>,
        mut records: Vec<CombatDeclarationRecord>,
        turn: &TurnScheduler,
        mut append: RulesJournal,
    ) -> Result<RuleProgress, EffectError> {
        self.pending_work = None;
        while !remaining.is_empty() {
            let player = remaining.remove(0);
            let attackers: Vec<_> = self
                .combat
                .as_ref()
                .ok_or(CombatError::InvalidContext)?
                .attackers()
                .filter(|a| a.defender() == player)
                .map(|a| a.combatant().object())
                .collect();
            if attackers.is_empty() {
                continue;
            }
            let blockers = self.eligible_blockers(player)?;
            if blockers.is_empty() {
                self.declare_blocks_at_cut(
                    player,
                    DeclarationOrigin::ForcedEmpty,
                    vec![],
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
            let frame = CombatDecision::Blockers(BlockersDecision::new(
                self.revision,
                player,
                blockers,
                attackers,
            ));
            self.pending_work = Some(PendingWork::Combat(CombatCursor {
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
        self.combat
            .as_mut()
            .ok_or(CombatError::InvalidContext)?
            .finish_absent_defenders(self.window.participants());
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
                    || current.window.turn_owner() != actor
                    || previous.combat.is_some()
                    || membership.len() != selected.len()
                    || membership
                        .iter()
                        .zip(&selected)
                        .any(|(member, declaration)| {
                            member.combatant().object() != declaration.attacker
                                || member.defender() != declaration.defender
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
        selected: Vec<crate::combat::BlockDeclaration>,
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
        self.combat
            .as_mut()
            .ok_or(CombatError::InvalidContext)?
            .declare_blocks(actor, participants)?;
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
        if &cursor.frame != frame || frame.revision() != self.revision {
            return Err(CombatError::WrongFrame.into());
        }
        if frame.chooser() != actor {
            return Err(EffectError::WrongChooser);
        }
        match (frame, choice) {
            (CombatDecision::Attackers(bound), DecisionReply::DeclareAttackers(mut selected)) => {
                self.require_combat_work(turn, TurnWork::DeclareAttackers)?;
                if actor != self.window.turn_owner() || self.combat.is_some() {
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
                }
                selected.sort_unstable_by_key(|a| a.attacker);
                let participants = selected
                    .iter()
                    .map(|a| {
                        let participant = self
                            .window
                            .cards()
                            .get(a.attacker)
                            .and_then(Combatant::from_object)
                            .ok_or(CombatError::IllegalAttacker)?;
                        Ok((participant, a.defender))
                    })
                    .collect::<Result<Vec<_>, CombatError>>()?;
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
                let before_declaration = next.rules_snapshot(Some(turn))?;
                next.combat = Some(CombatState::new(participants)?);
                next.emit_attackers_declaration(
                    &before_declaration,
                    turn,
                    actor,
                    DeclarationOrigin::PlayerReply,
                    selected.clone(),
                    &mut append,
                )?;
                let taps = changes
                    .into_iter()
                    .map(|c| AttackTap {
                        before: c.before,
                        after: c.after,
                    })
                    .collect();
                Ok(RuleProgress::new(
                    next,
                    RuleResult::Complete {
                        checks: CheckJournal::default(),
                        journal: DamageJournal::default(),
                        assignments: vec![],
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
            (CombatDecision::Blockers(bound), DecisionReply::DeclareBlockers(mut selected)) => {
                self.require_combat_work(turn, TurnWork::DeclareBlockers)?;
                let eligible = self.eligible_blockers(actor)?;
                let combat = self.combat.as_ref().ok_or(CombatError::InvalidContext)?;
                let mut seen = BTreeSet::new();
                for declaration in &selected {
                    if !seen.insert(declaration.blocker) {
                        return Err(CombatError::DuplicateBlocker.into());
                    }
                    if !bound.eligible_blockers().contains(&declaration.blocker)
                        || !eligible.contains(&declaration.blocker)
                    {
                        return Err(CombatError::IllegalBlocker.into());
                    }
                    if !bound.attackers().contains(&declaration.attacker)
                        || !combat
                            .attacker(declaration.attacker)
                            .is_some_and(|a| a.defender() == actor)
                    {
                        return Err(CombatError::IllegalBlockTarget.into());
                    }
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
                    actor,
                    DeclarationOrigin::PlayerReply,
                    selected.clone(),
                    participants,
                    turn,
                    &mut append,
                )?;
                next.advance_blocker_actors(
                    cursor.remaining_actors.clone(),
                    vec![CombatDeclarationRecord::Blockers {
                        player: actor,
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
                vec![],
                f.attackers().to_vec(),
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
