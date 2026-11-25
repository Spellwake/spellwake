//! Resuming pending rule decisions across casting, combat, and resolution.
use super::*;

impl EffectWindow {
    /// Resume the exact current owned rules process. A calendar is borrowed only
    /// from the native root; player replies supply no work-completion authority.
    #[inline(never)]
    pub fn resume_rule(
        &self,
        expected: u64,
        frame: &EffectDecision,
        actor: SeatId,
        reply: DecisionReply,
        turn: Option<&TurnScheduler>,
    ) -> Result<RuleProgress, EffectError> {
        match frame {
            EffectDecision::ManaColor(_) => Err(EffectError::WrongDecisionKind),
            EffectDecision::EntryChoice(frame)
                if self
                    .resolution_cursor()
                    .is_some_and(|cursor| cursor.revealed_piles.is_some()) =>
            {
                self.resume_pile_entry(expected, frame, actor, reply)
            }
            EffectDecision::ResolutionSelect(frame)
                if frame.operation() == ResolutionSelectionKind::ExileMilledCreatures =>
            {
                self.resume_milled_exile(expected, frame, actor, reply)
            }
            EffectDecision::ResolutionSelect(frame)
                if frame.operation() == ResolutionSelectionKind::VoteForCard =>
            {
                self.resume_graveyard_vote(expected, frame, actor, reply)
            }
            EffectDecision::ResolutionSelect(frame)
                if frame.operation() == ResolutionSelectionKind::FoundSpellToCast =>
            {
                self.resume_found_spell(expected, frame, actor, reply)
            }
            EffectDecision::ResolutionSelect(frame)
                if matches!(
                    frame.operation(),
                    ResolutionSelectionKind::EquipmentToToken
                        | ResolutionSelectionKind::TokenReceivingEquipment
                ) =>
            {
                self.resume_token_equipment(expected, frame, actor, reply)
            }
            EffectDecision::ResolutionSelect(frame)
                if frame.operation() == ResolutionSelectionKind::UnattachControlledEquipment =>
            {
                self.resume_equipment_protection(expected, frame, actor, reply)
            }
            EffectDecision::ResolutionSelect(frame)
                if frame.operation() == ResolutionSelectionKind::MeldPartner =>
            {
                self.resume_meld(expected, frame, actor, reply)
            }
            EffectDecision::ResolutionSelect(frame)
                if frame.operation() == ResolutionSelectionKind::Soulbond =>
            {
                self.resume_soulbond(expected, frame, actor, reply)
            }
            EffectDecision::ResolutionSelect(frame)
                if frame.operation() == ResolutionSelectionKind::TurnCreatureFaceUp =>
            {
                self.resume_turn_face_up_effect(expected, frame, actor, reply)
            }
            EffectDecision::ResolutionSelect(frame)
                if matches!(
                    frame.operation(),
                    ResolutionSelectionKind::GraveyardToBattlefield
                        | ResolutionSelectionKind::HandToBattlefield
                        | ResolutionSelectionKind::ExileToBattlefield
                ) =>
            {
                self.resume_hand_entry(expected, frame, actor, reply)
            }
            EffectDecision::Mutation(frame) => self.resume_mutation(expected, frame, actor, reply),
            EffectDecision::RevealedPiles(frame)
                if matches!(self.pending_work, Some(PendingWork::Damage(_))) =>
            {
                self.resume_damage_graveyard_order(expected, frame, actor, reply)
            }
            EffectDecision::RevealedPiles(frame) => {
                self.resume_pile_choice(expected, frame, actor, reply)
            }
            EffectDecision::ZoneReplacement(frame)
                if matches!(self.pending_work, Some(PendingWork::Damage(_))) =>
            {
                self.resume_damage_zone_followup(expected, frame, actor, reply)
            }
            EffectDecision::ZoneReplacement(frame)
                if self
                    .resolution_cursor()
                    .is_some_and(|cursor| cursor.revealed_piles.is_some()) =>
            {
                self.resume_pile_replacement(expected, frame, actor, reply)
            }
            EffectDecision::MyriadDefender(frame) => {
                let DecisionReply::MyriadDefender(defender) = reply else {
                    return Err(EffectError::WrongDecisionKind);
                };
                self.resume_myriad(expected, frame, actor, defender)
            }
            EffectDecision::ReselectAttackDefender(bound) => {
                let DecisionReply::ReselectAttackDefender(choice) = reply else {
                    return Err(EffectError::WrongDecisionKind);
                };
                self.resume_attack_defender(expected, bound, actor, choice)
            }
            EffectDecision::PlaneswalkerType(bound) => {
                let revision = self.next_revision(expected)?;
                let DecisionReply::PlaneswalkerType(kind) = reply else {
                    return Err(EffectError::WrongDecisionKind);
                };
                let entry = self
                    .window
                    .resolving()
                    .ok_or(EffectError::WrongResolutionFrame)?;
                if actor != bound.chooser || actor != entry.controller() {
                    return Err(EffectError::WrongChooser);
                }
                if bound.revision != self.revision
                    || bound.entry != entry.id()
                    || Some(bound.instruction) != self.instruction_index()
                    || self.current_instruction()? != &Instruction::ChoosePlaneswalkerTypeForCopies
                {
                    return Err(EffectError::WrongResolutionFrame);
                }
                let mut next = self.clone();
                next.revision = revision;
                next.watch_ability_copy(
                    entry,
                    crate::occurrence::AbilityCopyWatch::PlaneswalkerType(kind),
                )?;
                next.advance_instruction()?;
                next.progress_rules(
                    DamageJournal::default(),
                    vec![],
                    vec![],
                    DrawJournal::default(),
                    RulesJournal::default(),
                )
            }
            EffectDecision::AttackDirection(bound) => {
                let revision = self.next_revision(expected)?;
                let DecisionReply::AttackDirection(direction) = reply else {
                    return Err(EffectError::WrongDecisionKind);
                };
                let cursor = self
                    .resolution_cursor()
                    .ok_or(EffectError::WrongResolutionFrame)?;
                let entry = self
                    .window
                    .resolving()
                    .ok_or(EffectError::WrongResolutionFrame)?;
                if actor != bound.chooser || actor != entry.controller() {
                    return Err(EffectError::WrongChooser);
                }
                if bound.revision != self.revision
                    || bound.entry != entry.id()
                    || bound.instruction != cursor.instruction
                    || self.current_instruction()? != &Instruction::ChooseAttackDirection
                {
                    return Err(EffectError::WrongResolutionFrame);
                }
                let mut next = self.clone();
                next.revision = revision;
                next.attack_directions.push((actor, direction));
                next.advance_instruction()?;
                next.progress_rules(
                    DamageJournal::default(),
                    vec![],
                    vec![],
                    DrawJournal::default(),
                    RulesJournal::default(),
                )
            }
            actual @ (EffectDecision::DrawReplacement(_)
            | EffectDecision::ZoneReplacement(_)
            | EffectDecision::OptionalEffect(_))
                if self.draw_decision().as_ref() == Some(actual) =>
            {
                let mut next = self.clone();
                next.accept_draw_decision(expected, actual, actor, reply)?;
                next.progress_rules(
                    DamageJournal::default(),
                    vec![],
                    vec![],
                    DrawJournal::default(),
                    RulesJournal::default(),
                )
            }
            EffectDecision::DrawReplacement(_) => Err(EffectError::WrongResolutionFrame),
            EffectDecision::OpponentChoice(bound) => {
                let revision = self.next_revision(expected)?;
                let DecisionReply::OpponentChoice(player) = reply else {
                    return Err(EffectError::WrongDecisionKind);
                };
                let cursor = self
                    .resolution_cursor()
                    .ok_or(EffectError::WrongResolutionFrame)?;
                let entry = self
                    .window
                    .resolving()
                    .ok_or(EffectError::WrongResolutionFrame)?;
                if actor != bound.chooser || actor != entry.controller() {
                    return Err(EffectError::WrongChooser);
                }
                if bound.revision != self.revision
                    || bound.entry != entry.id()
                    || bound.instruction != cursor.instruction
                    || cursor.chosen_opponent.is_some()
                    || !matches!(
                        self.current_instruction()?,
                        Instruction::DrawToChosenOpponentHandSize
                            | Instruction::LookTopOneRestGraveyard { .. }
                            | Instruction::RevealTopHandRestGraveyard { .. }
                            | Instruction::LookTopOneRestBottom { .. }
                            | Instruction::RevealTopCreaturesAndLandsRestBottom { .. }
                            | Instruction::RevealHandAndBottomPiles
                            | Instruction::RevealDividePiles
                            | Instruction::RevealOpponentChoosesCreature
                            | Instruction::ChooseValue {
                                domain: crate::program::ResolutionChoiceDomain::Player {
                                    opponents_only: true,
                                },
                                ..
                            }
                            | Instruction::Clash { .. }
                    )
                    || self
                        .combat_rules
                        .opponents(self.window.participants(), actor)?
                        != bound.opponents
                    || !bound.opponents.contains(&player)
                {
                    return Err(EffectError::WrongResolutionFrame);
                }
                let mut next = self.clone();
                next.revision = revision;
                next.resolution_cursor_mut()
                    .ok_or(EffectError::WrongResolutionFrame)?
                    .chosen_opponent = Some(player);
                next.progress_rules(
                    DamageJournal::default(),
                    vec![],
                    vec![],
                    DrawJournal::default(),
                    RulesJournal::default(),
                )
            }
            EffectDecision::DamageRecipientChoice(bound) => {
                let revision = self.next_revision(expected)?;
                let DecisionReply::ChooseDamageRecipient(recipient) = reply else {
                    return Err(EffectError::WrongDecisionKind);
                };
                let cursor = self
                    .resolution_cursor()
                    .ok_or(EffectError::WrongResolutionFrame)?;
                let entry = self
                    .window
                    .resolving()
                    .ok_or(EffectError::WrongResolutionFrame)?;
                if bound.revision != expected
                    || bound.chooser != actor
                    || entry.controller() != actor
                    || bound.entry != cursor.entry
                    || bound.instruction != cursor.instruction
                    || cursor.chosen_damage_recipient.is_some()
                    || !bound.recipients.contains(&recipient)
                {
                    return Err(EffectError::WrongResolutionFrame);
                }
                let mut next = self.clone();
                next.revision = revision;
                next.resolution_cursor_mut()
                    .ok_or(EffectError::WrongResolutionFrame)?
                    .chosen_damage_recipient = Some(recipient);
                next.progress_rules(
                    DamageJournal::default(),
                    vec![],
                    vec![],
                    DrawJournal::default(),
                    RulesJournal::default(),
                )
            }
            EffectDecision::LoyaltyRemoval(bound) => {
                let revision = self.next_revision(expected)?;
                let DecisionReply::SelectResolution(objects) = reply else {
                    return Err(EffectError::WrongDecisionKind);
                };
                let cursor = self
                    .resolution_cursor()
                    .ok_or(EffectError::WrongResolutionFrame)?;
                if bound.revision != self.revision
                    || bound.entry != cursor.entry
                    || bound.instruction != cursor.instruction
                    || bound.chooser != actor
                    || !matches!(
                        self.current_instruction()?,
                        Instruction::RemoveLoyaltyExileTopForPlay
                    )
                    || cursor.chosen_loyalty_removals.is_some()
                    || objects.iter().copied().collect::<BTreeSet<_>>().len() != objects.len()
                    || objects.iter().any(|id| !bound.objects.contains(id))
                {
                    return Err(EffectError::WrongResolutionFrame);
                }
                let mut next = self.clone();
                next.revision = revision;
                next.resolution_cursor_mut()
                    .ok_or(EffectError::WrongResolutionFrame)?
                    .chosen_loyalty_removals = Some(objects);
                next.progress_rules(
                    DamageJournal::default(),
                    vec![],
                    vec![],
                    DrawJournal::default(),
                    RulesJournal::default(),
                )
            }
            EffectDecision::ResolutionManaColor(bound) => {
                let revision = self.next_revision(expected)?;
                let cursor = self
                    .resolution_cursor()
                    .ok_or(EffectError::WrongResolutionFrame)?;
                let actual = self
                    .window
                    .resolving()
                    .ok_or(EffectError::WrongResolutionFrame)?;
                if bound.revision != self.revision
                    || bound.entry != cursor.entry
                    || bound.instruction != cursor.instruction
                    || bound.chooser != actor
                    || actual.controller() != actor
                    || cursor.chosen_effect_color.is_some()
                    || cursor.chosen_mana_split.is_some()
                {
                    return Err(EffectError::WrongResolutionFrame);
                }
                let mut next = self.clone();
                next.revision = revision;
                let cursor = next
                    .resolution_cursor_mut()
                    .ok_or(EffectError::WrongResolutionFrame)?;
                let current = self.current_instruction()?;
                let effective_mana_yield = match current {
                    Instruction::AddMana(
                        crate::program::EffectManaYield::RetainedUntilCleanup(inner),
                    ) => Some(inner.as_ref()),
                    Instruction::AddMana(inner) => Some(inner),
                    _ => None,
                };
                match (current, effective_mana_yield, reply) {
                    (
                        _,
                        Some(crate::program::EffectManaYield::Split {
                            first,
                            second,
                            amount,
                        }),
                        DecisionReply::SplitMana(portion),
                    ) => {
                        let total = self.resolve_value_expr(actor, amount)?;
                        if !total.is_positive()
                            || portion < 0.into()
                            || portion > total
                            || bound.split() != Some((*first, *second, &total))
                        {
                            return Err(EffectError::WrongResolutionFrame);
                        }
                        cursor.chosen_mana_split = Some(portion);
                    }
                    (instruction, effective_mana_yield, DecisionReply::ManaColor(color)) => {
                        if bound.split().is_some()
                            || !bound.colors.contains(&color)
                            || !match (instruction, effective_mana_yield) {
                                (
                                    _,
                                    Some(
                                        crate::program::EffectManaYield::RestrictedChoice {
                                            colors,
                                            ..
                                        }
                                        | crate::program::EffectManaYield::Choice { colors, .. },
                                    ),
                                ) => colors == &bound.colors,
                                (
                                    _,
                                    Some(
                                        crate::program::EffectManaYield::LandProducible { .. }
                                        | crate::program::EffectManaYield::CommanderIdentity {
                                            ..
                                        }
                                        | crate::program::EffectManaYield::DistinctColors { .. },
                                    ),
                                ) => !bound.colors.is_empty(),
                                (
                                    Instruction::GrantProtectionChosenColor(_)
                                    | Instruction::GrantChosenProtection { .. },
                                    _,
                                ) => bound.colors == mana_domains::colored_mana(),
                                _ => false,
                            }
                        {
                            return Err(EffectError::WrongResolutionFrame);
                        }
                        cursor.chosen_effect_color = Some(color);
                    }
                    _ => return Err(EffectError::WrongDecisionKind),
                }
                next.progress_rules(
                    DamageJournal::default(),
                    vec![],
                    vec![],
                    DrawJournal::default(),
                    RulesJournal::default(),
                )
            }
            EffectDecision::OptionIndex(bound) => {
                let revision = self.next_revision(expected)?;
                let DecisionReply::OptionIndex(chosen) = reply else {
                    return Err(EffectError::WrongDecisionKind);
                };
                let cursor = self
                    .resolution_cursor()
                    .ok_or(EffectError::WrongResolutionFrame)?;
                if bound.revision != self.revision
                    || bound.entry != cursor.entry
                    || bound.instruction != cursor.instruction
                    || bound.chooser != actor
                    || chosen >= bound.count
                    || cursor.chosen_option_index.is_some()
                {
                    return Err(EffectError::WrongResolutionFrame);
                }
                let mut next = self.clone();
                next.revision = revision;
                next.resolution_cursor_mut()
                    .ok_or(EffectError::WrongResolutionFrame)?
                    .chosen_option_index = Some(chosen);
                next.progress_rules(
                    DamageJournal::default(),
                    vec![],
                    vec![],
                    DrawJournal::default(),
                    RulesJournal::default(),
                )
            }

            EffectDecision::ZoneReplacement(bound)
                if matches!(self.pending_work, Some(PendingWork::TokenEntry(_))) =>
            {
                self.resume_token_copy(expected, bound, actor, reply)
            }
            EffectDecision::ZoneReplacement(bound) if matches!(&self.pending_work, Some(PendingWork::ManaSacrifice(work)) if work.batch.is_some()) =>
            {
                let revision = self.next_revision(expected)?;
                let DecisionReply::ZoneReplacement(reply) = reply else {
                    return Err(EffectError::WrongDecisionKind);
                };
                let mut next = self.clone();
                let Some(PendingWork::ManaSacrifice(work)) = &mut next.pending_work else {
                    return Err(EffectError::WrongPhase);
                };
                work.batch.as_mut().ok_or(EffectError::WrongPhase)?.accept(
                    self.revision,
                    bound,
                    actor,
                    reply,
                )?;
                next.revision = revision;
                next.progress_rules(
                    DamageJournal::default(),
                    vec![],
                    vec![],
                    DrawJournal::default(),
                    RulesJournal::default(),
                )
            }
            EffectDecision::ZoneReplacement(bound)
                if matches!(self.pending_work, Some(PendingWork::PermanentEntry(_))) =>
            {
                let revision = self.next_revision(expected)?;
                let DecisionReply::ZoneReplacement(reply) = reply else {
                    return Err(EffectError::WrongDecisionKind);
                };
                let mut next = self.clone();
                let Some(PendingWork::PermanentEntry(work)) = &mut next.pending_work else {
                    return Err(EffectError::WrongResolutionFrame);
                };
                work.batch.accept(self.revision, bound, actor, reply)?;
                next.revision = revision;
                next.progress_permanent_entry()
            }
            EffectDecision::ZoneReplacement(bound) if self.cast_cost_replacement.is_some() => {
                let revision = self.next_revision(expected)?;
                let DecisionReply::ZoneReplacement(reply) = reply else {
                    return Err(EffectError::WrongDecisionKind);
                };
                let mut next = self.clone();
                let mut work = next
                    .cast_cost_replacement
                    .take()
                    .ok_or(EffectError::WrongResolutionFrame)?;
                work.batch.accept(self.revision, bound, actor, reply)?;
                next.revision = revision;
                if let Some(frame) = work.batch.progress(revision)? {
                    next.cast_cost_replacement = Some(work);
                    return Ok(RuleProgress::new(
                        next,
                        RuleResult::Pending {
                            checks: CheckJournal::default(),
                            frame,
                            journal: DamageJournal::default(),
                            declarations: vec![],
                            assignments: vec![],
                            draws: DrawJournal::default(),
                        },
                        RulesJournal::default(),
                    ));
                }
                next.cast_cost_replacement = Some(work);
                if let Some(frame) = next.pending_cast_cost_decision() {
                    return Ok(RuleProgress::new(
                        next,
                        RuleResult::Pending {
                            checks: CheckJournal::default(),
                            frame,
                            journal: DamageJournal::default(),
                            declarations: vec![],
                            assignments: vec![],
                            draws: DrawJournal::default(),
                        },
                        RulesJournal::default(),
                    ));
                }
                next.finish_cast_cost_replacement()?;
                next.continue_linked_returns(turn)
            }
            EffectDecision::CastRecovery(bound) => {
                let revision = self.next_revision(expected)?;
                if self.pending_resolution_decision().as_ref() != Some(frame) {
                    return Err(EffectError::WrongResolutionFrame);
                }
                if actor != bound.chooser {
                    return Err(EffectError::WrongChooser);
                }
                let DecisionReply::CastRecovery(choice) = reply else {
                    return Err(EffectError::WrongDecisionKind);
                };
                let mut next = self.clone();
                next.revision = revision;
                let mut deferred = next
                    .deferred_mana_cast
                    .take()
                    .ok_or(EffectError::InconsistentProgram)?;
                let mut cursor = deferred
                    .parent_cursor
                    .take()
                    .ok_or(EffectError::InconsistentProgram)?;
                let mut append = RulesJournal::default();
                match choice {
                    CastRecoveryChoice::Retry(payment) => {
                        deferred.prepared.payment.mana = payment;
                        deferred.prepared.payment.restricted_mana.clear();
                        if matches!(
                            deferred.kind,
                            ExileCastAbility::RevealedLibrary | ExileCastAbility::Impulse
                        ) {
                            cursor.impulse.decided = true;
                            cursor.impulse.cast = true;
                        }
                        next.pending_work = Some(PendingWork::Resolution(cursor));
                        let (state, receipt, cast_append) = next
                            .finish_cast_payment(
                                deferred.prepared,
                                deferred.turn.as_ref(),
                                RulesJournal::default(),
                            )?
                            .into_parts();
                        next = state;
                        append.append(cast_append);
                        if next.deferred_cast.is_none() {
                            next.completed_deferred_receipts = Some((receipt, None));
                        }
                    }
                    CastRecoveryChoice::RetryRestricted {
                        payment,
                        restricted_mana,
                    } => {
                        deferred.prepared.payment.mana = payment;
                        deferred.prepared.payment.restricted_mana = restricted_mana;
                        if matches!(
                            deferred.kind,
                            ExileCastAbility::RevealedLibrary | ExileCastAbility::Impulse
                        ) {
                            cursor.impulse.decided = true;
                            cursor.impulse.cast = true;
                        }
                        next.pending_work = Some(PendingWork::Resolution(cursor));
                        let (state, receipt, cast_append) = next
                            .finish_cast_payment(
                                deferred.prepared,
                                deferred.turn.as_ref(),
                                RulesJournal::default(),
                            )?
                            .into_parts();
                        next = state;
                        append.append(cast_append);
                        if next.deferred_cast.is_none() {
                            next.completed_deferred_receipts = Some((receipt, None));
                        }
                    }
                    CastRecoveryChoice::Abandon => {
                        let original = deferred.prepared.original_spell;
                        next.window = next.window.reverse_child_cast(
                            deferred.parent_entry,
                            deferred.prepared.entry,
                            &original,
                            &deferred.prepared.graveyard_below,
                        )?;
                        next.payloads.remove(&deferred.prepared.entry);
                        next.typed_exile_pending.remove(&deferred.prepared.entry);
                        next.characteristics.suspend_haste.remove(&bound.spell);
                        next.characteristics.bestowed_spells.remove(&bound.spell);
                        if let Some(value) = deferred.prepared.foretold_at {
                            next.characteristics.foretold.insert(original.id(), value);
                        }
                        if let Some(value) = deferred.prepared.plotted_at {
                            next.plotted.insert(original.id(), value);
                        }
                        if let Some(value) = deferred.prepared.warped_at {
                            next.warped.insert(original.id(), value);
                        }
                        if let Some(value) = deferred.prepared.time_counters {
                            next.characteristics
                                .time_counters
                                .insert(original.id(), value);
                        }
                        next.triggers
                            .reverse_cast_announcement(deferred.prepared.announcement_event);
                        append.records.push(
                            crate::occurrence::RulesRecord::CastAnnouncementReversed {
                                entry: deferred.prepared.entry,
                                stack_object: bound.spell,
                                restored: original.id(),
                            },
                        );
                        if matches!(
                            deferred.kind,
                            ExileCastAbility::RevealedLibrary | ExileCastAbility::Impulse
                        ) {
                            cursor.impulse.decided = true;
                            cursor.impulse.cast = false;
                        }
                        if deferred.kind == ExileCastAbility::Madness {
                            next.finish_declined_madness(&cursor, &mut append)?;
                        }
                        next.pending_work = Some(PendingWork::Resolution(cursor));
                    }
                }
                next.progress_rules(
                    DamageJournal::default(),
                    vec![],
                    vec![],
                    DrawJournal::default(),
                    append,
                )
            }
            EffectDecision::ZoneReplacement(bound) if self.linked_return_work.is_some() => {
                let revision = self.next_revision(expected)?;
                if self.linked_return_decision()?.as_ref() != Some(frame) {
                    return Err(EffectError::WrongResolutionFrame);
                }
                let DecisionReply::ZoneReplacement(reply) = reply else {
                    return Err(EffectError::WrongDecisionKind);
                };
                let mut next = self.clone();
                let batch = next
                    .linked_return_work
                    .as_mut()
                    .and_then(|work| work.replacements.as_mut())
                    .ok_or(EffectError::WrongResolutionFrame)?;
                batch.accept(self.revision, bound, actor, reply)?;
                next.revision = revision;
                next.continue_linked_returns(turn)
            }
            EffectDecision::ZoneReplacement(bound)
                if matches!(self.pending_work, Some(PendingWork::LookTop(_))) =>
            {
                let revision = self.next_revision(expected)?;
                let DecisionReply::ZoneReplacement(reply) = reply else {
                    return Err(EffectError::WrongDecisionKind);
                };
                let mut next = self.clone();
                let Some(PendingWork::LookTop(work)) = &mut next.pending_work else {
                    return Err(EffectError::WrongResolutionFrame);
                };
                work.replacement
                    .as_mut()
                    .ok_or(EffectError::WrongResolutionFrame)?
                    .accept(self.revision, bound, actor, reply)?;
                next.revision = revision;
                next.progress_look_top(
                    DamageJournal::default(),
                    vec![],
                    vec![],
                    DrawJournal::default(),
                    RulesJournal::default(),
                )
            }
            EffectDecision::ZoneReplacement(bound)
                if matches!(self.pending_work, Some(PendingWork::ActivationCost(_))) =>
            {
                let revision = self.next_revision(expected)?;
                let DecisionReply::ZoneReplacement(reply) = reply else {
                    return Err(EffectError::WrongDecisionKind);
                };
                let mut next = self.clone();
                let Some(PendingWork::ActivationCost(work)) = &mut next.pending_work else {
                    return Err(EffectError::WrongResolutionFrame);
                };
                work.batch.accept(self.revision, bound, actor, reply)?;
                next.revision = revision;
                next.progress_activation_cost(turn)
            }
            EffectDecision::ZoneReplacement(bound)
                if matches!(self.pending_work, Some(PendingWork::ImpulseReplacement(_))) =>
            {
                let revision = self.next_revision(expected)?;
                let DecisionReply::ZoneReplacement(reply) = reply else {
                    return Err(EffectError::WrongDecisionKind);
                };
                let mut next = self.clone();
                let Some(PendingWork::ImpulseReplacement(work)) = &mut next.pending_work else {
                    return Err(EffectError::WrongResolutionFrame);
                };
                work.batch.accept(self.revision, bound, actor, reply)?;
                next.revision = revision;
                next.progress_rules(
                    DamageJournal::default(),
                    vec![],
                    vec![],
                    DrawJournal::default(),
                    RulesJournal::default(),
                )
            }
            EffectDecision::ZoneReplacement(bound)
                if matches!(self.pending_work, Some(PendingWork::GraveyardShuffle(_))) =>
            {
                let revision = self.next_revision(expected)?;
                let DecisionReply::ZoneReplacement(reply) = reply else {
                    return Err(EffectError::WrongDecisionKind);
                };
                let mut next = self.clone();
                let Some(PendingWork::GraveyardShuffle(work)) = &mut next.pending_work else {
                    return Err(EffectError::WrongResolutionFrame);
                };
                work.batch.accept(self.revision, bound, actor, reply)?;
                next.revision = revision;
                next.progress_rules(
                    DamageJournal::default(),
                    vec![],
                    vec![],
                    DrawJournal::default(),
                    RulesJournal::default(),
                )
            }
            EffectDecision::ZoneReplacement(bound)
                if matches!(self.pending_work, Some(PendingWork::GraveyardExile(_))) =>
            {
                let revision = self.next_revision(expected)?;
                let DecisionReply::ZoneReplacement(reply) = reply else {
                    return Err(EffectError::WrongDecisionKind);
                };
                let mut next = self.clone();
                let Some(PendingWork::GraveyardExile(work)) = &mut next.pending_work else {
                    return Err(EffectError::WrongResolutionFrame);
                };
                work.batch.accept(self.revision, bound, actor, reply)?;
                next.revision = revision;
                next.progress_rules(
                    DamageJournal::default(),
                    vec![],
                    vec![],
                    DrawJournal::default(),
                    RulesJournal::default(),
                )
            }
            EffectDecision::ZoneReplacement(bound)
                if matches!(
                    self.pending_work,
                    Some(PendingWork::SelectedActivationCost(_))
                ) =>
            {
                let revision = self.next_revision(expected)?;
                let DecisionReply::ZoneReplacement(reply) = reply else {
                    return Err(EffectError::WrongDecisionKind);
                };
                let mut next = self.clone();
                let Some(PendingWork::SelectedActivationCost(work)) = &mut next.pending_work else {
                    return Err(EffectError::WrongResolutionFrame);
                };
                work.batch.accept(self.revision, bound, actor, reply)?;
                next.revision = revision;
                next.progress_selected_activation_cost(turn)
            }
            EffectDecision::ZoneReplacement(bound)
                if matches!(self.pending_work, Some(PendingWork::LandPlay(_))) =>
            {
                let revision = self.next_revision(expected)?;
                let DecisionReply::ZoneReplacement(reply) = reply else {
                    return Err(EffectError::WrongDecisionKind);
                };
                let mut next = self.clone();
                let Some(PendingWork::LandPlay(work)) = &mut next.pending_work else {
                    return Err(EffectError::WrongResolutionFrame);
                };
                work.batch.accept(self.revision, bound, actor, reply)?;
                next.revision = revision;
                next.progress_land_play()
            }
            EffectDecision::ZoneReplacement(bound)
                if matches!(self.pending_work, Some(PendingWork::SearchEntry(_))) =>
            {
                let revision = self.next_revision(expected)?;
                let DecisionReply::ZoneReplacement(reply) = reply else {
                    return Err(EffectError::WrongDecisionKind);
                };
                let mut next = self.clone();
                let Some(PendingWork::SearchEntry(work)) = &mut next.pending_work else {
                    return Err(EffectError::WrongResolutionFrame);
                };
                work.batch.accept(self.revision, bound, actor, reply)?;
                next.revision = revision;
                next.progress_rules(
                    DamageJournal::default(),
                    vec![],
                    vec![],
                    DrawJournal::default(),
                    RulesJournal::default(),
                )
            }
            EffectDecision::ZoneReplacement(bound)
                if matches!(self.pending_work, Some(PendingWork::BattlefieldEntry(_))) =>
            {
                let revision = self.next_revision(expected)?;
                let DecisionReply::ZoneReplacement(reply) = reply else {
                    return Err(EffectError::WrongDecisionKind);
                };
                let mut next = self.clone();
                let Some(PendingWork::BattlefieldEntry(work)) = &mut next.pending_work else {
                    return Err(EffectError::WrongResolutionFrame);
                };
                work.batch.accept(self.revision, bound, actor, reply)?;
                next.revision = revision;
                next.progress_rules(
                    DamageJournal::default(),
                    vec![],
                    vec![],
                    DrawJournal::default(),
                    RulesJournal::default(),
                )
            }
            EffectDecision::LinkedAuraChoice(bound) => {
                let revision = self.next_revision(expected)?;
                let linked = self.linked_return_work.is_some();
                if linked {
                    if self.linked_return_decision()?.as_ref() != Some(frame) {
                        return Err(EffectError::WrongResolutionFrame);
                    }
                } else if let Some(work) = self
                    .resolution_cursor()
                    .and_then(|cursor| cursor.revealed_piles.as_ref())
                {
                    if work.aura_offer.as_ref() != Some(bound) || bound.revision() != expected {
                        return Err(EffectError::WrongResolutionFrame);
                    }
                } else if let Some(PendingWork::SearchEntry(work)) = &self.pending_work {
                    if self.search_aura_frame(work)?.as_ref() != Some(bound) {
                        return Err(EffectError::WrongResolutionFrame);
                    }
                } else if let Some(entry) = self
                    .window
                    .resolving()
                    .filter(|entry| self.window.spell_object(entry) == Some(bound.object))
                {
                    if self.copied_spell_aura_frame(entry, bound.object)?.as_ref() != Some(bound) {
                        return Err(EffectError::WrongResolutionFrame);
                    }
                } else if self.graveyard_aura_entry_frame()?.as_ref() != Some(bound) {
                    return Err(EffectError::WrongResolutionFrame);
                }
                if actor != bound.chooser {
                    return Err(EffectError::WrongChooser);
                }
                let DecisionReply::LinkedAuraChoice(host) = reply else {
                    return Err(EffectError::WrongDecisionKind);
                };
                if !bound.domain.contains(&host) {
                    return Err(EffectError::IllegalTarget);
                }
                let mut next = self.clone();
                next.revision = revision;
                if linked {
                    next.linked_return_work
                        .as_mut()
                        .ok_or(EffectError::WrongResolutionFrame)?
                        .auras
                        .insert(bound.object, host);
                    next.continue_linked_returns(turn)
                } else {
                    if let Some(work) = next
                        .resolution_cursor_mut()
                        .and_then(|cursor| cursor.revealed_piles.as_mut())
                    {
                        work.aura_offer = None;
                    }
                    next.resolution_aura_choices.insert(bound.object, host);
                    if let Some(entry) = next
                        .window
                        .resolving()
                        .filter(|entry| next.window.spell_object(entry) == Some(bound.object))
                        .cloned()
                    {
                        return next.resolve_permanent_entry(&entry);
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
            EffectDecision::LinkedReturnOrder(bound)
                if matches!(self.pending_work, Some(PendingWork::LookTop(_))) =>
            {
                self.resume_looked_order(expected, bound, actor, reply)
            }
            EffectDecision::LinkedReturnOrder(bound) => {
                let revision = self.next_revision(expected)?;
                if let Some(PendingWork::ImpulseReplacement(work)) = &self.pending_work {
                    if self.impulse_graveyard_order(work)?.as_ref() != Some(bound)
                        || actor != bound.chooser
                    {
                        return Err(EffectError::WrongResolutionFrame);
                    }
                    let DecisionReply::LinkedReturnOrder(order) = reply else {
                        return Err(EffectError::WrongDecisionKind);
                    };
                    if order.len() != bound.objects.len()
                        || order.iter().copied().collect::<BTreeSet<_>>()
                            != bound.objects.iter().copied().collect()
                    {
                        return Err(EffectError::IllegalTimestampOrder);
                    }
                    let mut next = self.clone();
                    next.revision = revision;
                    let Some(PendingWork::ImpulseReplacement(work)) = &mut next.pending_work else {
                        return Err(EffectError::WrongResolutionFrame);
                    };
                    work.graveyard_orders.insert(actor, order);
                    return next.progress_rules(
                        DamageJournal::default(),
                        vec![],
                        vec![],
                        DrawJournal::default(),
                        RulesJournal::default(),
                    );
                }
                if let Some(PendingWork::SelectedActivationCost(work)) = &self.pending_work {
                    if self.selected_activation_cost_frame(work)?.as_ref() != Some(frame) {
                        return Err(EffectError::WrongResolutionFrame);
                    }
                    if actor != bound.chooser {
                        return Err(EffectError::WrongChooser);
                    }
                    let DecisionReply::LinkedReturnOrder(order) = reply else {
                        return Err(EffectError::WrongDecisionKind);
                    };
                    if order.len() != bound.objects.len()
                        || order.iter().copied().collect::<BTreeSet<_>>()
                            != bound.objects.iter().copied().collect()
                    {
                        return Err(EffectError::IllegalTimestampOrder);
                    }
                    let mut next = self.clone();
                    next.revision = revision;
                    let Some(PendingWork::SelectedActivationCost(work)) = &mut next.pending_work
                    else {
                        return Err(EffectError::WrongResolutionFrame);
                    };
                    work.graveyard_orders.insert(actor, order);
                    return next.progress_selected_activation_cost(turn);
                }
                if matches!(self.pending_work, Some(PendingWork::SearchEntry(_))) {
                    let DecisionReply::LinkedReturnOrder(order) = reply else {
                        return Err(EffectError::WrongDecisionKind);
                    };
                    if actor != bound.chooser {
                        return Err(EffectError::WrongChooser);
                    }
                    if order.len() != bound.objects.len()
                        || order.iter().copied().collect::<BTreeSet<_>>()
                            != bound.objects.iter().copied().collect()
                    {
                        return Err(EffectError::IllegalTimestampOrder);
                    }
                    let mut next = self.clone();
                    let Some(PendingWork::SearchEntry(work)) = &mut next.pending_work else {
                        return Err(EffectError::WrongResolutionFrame);
                    };
                    if work.graveyard_frame(self.revision, &self.window)? != Some(bound.clone()) {
                        return Err(EffectError::WrongResolutionFrame);
                    }
                    work.graveyard_order = Some(order);
                    next.revision = revision;
                    return next.progress_rules(
                        DamageJournal::default(),
                        vec![],
                        vec![],
                        DrawJournal::default(),
                        RulesJournal::default(),
                    );
                }
                if self.cast_cost_replacement.is_some() {
                    if self.pending_cast_cost_decision().as_ref() != Some(frame) {
                        return Err(EffectError::WrongResolutionFrame);
                    }
                    if actor != bound.chooser {
                        return Err(EffectError::WrongChooser);
                    }
                    let DecisionReply::LinkedReturnOrder(order) = reply else {
                        return Err(EffectError::WrongDecisionKind);
                    };
                    if order.len() != bound.objects.len()
                        || order.iter().copied().collect::<BTreeSet<_>>()
                            != bound.objects.iter().copied().collect()
                    {
                        return Err(EffectError::IllegalTimestampOrder);
                    }
                    let mut next = self.clone();
                    next.revision = revision;
                    next.cast_cost_replacement
                        .as_mut()
                        .ok_or(EffectError::WrongResolutionFrame)?
                        .graveyard_orders
                        .insert(actor, order);
                    if let Some(frame) = next.pending_cast_cost_decision() {
                        return Ok(RuleProgress::new(
                            next,
                            RuleResult::Pending {
                                checks: CheckJournal::default(),
                                frame,
                                journal: DamageJournal::default(),
                                declarations: vec![],
                                assignments: vec![],
                                draws: DrawJournal::default(),
                            },
                            RulesJournal::default(),
                        ));
                    }
                    next.finish_cast_cost_replacement()?;
                    return next.continue_linked_returns(turn);
                }
                if self.linked_return_decision()?.as_ref() != Some(frame) {
                    return Err(EffectError::WrongResolutionFrame);
                }
                if actor != bound.chooser {
                    return Err(EffectError::WrongChooser);
                }
                let DecisionReply::LinkedReturnOrder(order) = reply else {
                    return Err(EffectError::WrongDecisionKind);
                };
                if order.len() != bound.objects.len()
                    || order.iter().copied().collect::<BTreeSet<_>>()
                        != bound.objects.iter().copied().collect()
                {
                    return Err(EffectError::IllegalTimestampOrder);
                }
                let mut next = self.clone();
                next.revision = revision;
                let work = next
                    .linked_return_work
                    .as_mut()
                    .ok_or(EffectError::WrongResolutionFrame)?;
                if bound.destination == crate::program::PublicZoneDestination::Graveyard {
                    work.graveyard_orders.insert(actor, order);
                } else {
                    work.orders.insert(actor, order);
                }
                next.continue_linked_returns(turn)
            }
            EffectDecision::TokenAuraChoice(bound) => {
                let DecisionReply::TokenAuraChoice(host) = reply else {
                    return Err(EffectError::WrongDecisionKind);
                };
                self.accept_token_aura(expected, bound, actor, host)?
                    .progress_rules(
                        DamageJournal::default(),
                        vec![],
                        vec![],
                        DrawJournal::default(),
                        RulesJournal::default(),
                    )
            }
            EffectDecision::TokenEntryChoice(bound) => {
                let DecisionReply::TokenEntryChoice(choice) = reply else {
                    return Err(EffectError::WrongDecisionKind);
                };
                self.accept_token_entry_choice(expected, bound, actor, choice)?
                    .progress_rules(
                        DamageJournal::default(),
                        vec![],
                        vec![],
                        DrawJournal::default(),
                        RulesJournal::default(),
                    )
            }
            EffectDecision::EntryChoice(bound)
                if matches!(self.pending_work, Some(PendingWork::SearchEntry(_))) =>
            {
                let revision = self.next_revision(expected)?;
                let Some(PendingWork::SearchEntry(work)) = &self.pending_work else {
                    return Err(EffectError::WrongResolutionFrame);
                };
                if self.search_entry_choice_frame(work)?.as_ref() != Some(bound) {
                    return Err(EffectError::WrongResolutionFrame);
                }
                if actor != bound.chooser {
                    return Err(EffectError::WrongChooser);
                }
                let DecisionReply::EntryChoice(choice) = reply else {
                    return Err(EffectError::WrongDecisionKind);
                };
                if !self.valid_entry_choice(bound, choice) {
                    return Err(EffectError::IllegalTarget);
                }
                let mut next = self.clone();
                next.revision = revision;
                let Some(PendingWork::SearchEntry(work)) = &mut next.pending_work else {
                    return Err(EffectError::WrongResolutionFrame);
                };
                match choice {
                    crate::land::EntryChoice::Players(_)
                    | crate::land::EntryChoice::CardName(_)
                    | crate::land::EntryChoice::PayLife(_)
                    | crate::land::EntryChoice::OptionIndex(_) => {
                        next.resolution_entry_choices.insert(bound.object, choice);
                    }
                    crate::land::EntryChoice::Opponent(_) => {
                        if matches!(
                            bound.rule,
                            crate::land::EntryTap::ChooseOpponent
                                | crate::land::EntryTap::ChoosePlayer
                        ) {
                            next.resolution_entry_choices.insert(bound.object, choice);
                        } else {
                            return Err(EffectError::WrongDecisionKind);
                        }
                    }
                    crate::land::EntryChoice::Color(color) => {
                        work.colors.push((bound.object, color))
                    }
                    crate::land::EntryChoice::Reveal(object) => {
                        work.reveals.push((bound.object, object))
                    }
                    crate::land::EntryChoice::Chapter(n) => work.chapters.push((bound.object, n)),
                    crate::land::EntryChoice::CreatureType(kind) => {
                        work.creature_types.push((bound.object, kind))
                    }
                }
                next.progress_rules(
                    DamageJournal::default(),
                    vec![],
                    vec![],
                    DrawJournal::default(),
                    RulesJournal::default(),
                )
            }
            EffectDecision::EntryChoice(bound)
                if matches!(self.pending_work, Some(PendingWork::LandPlay(_))) =>
            {
                let revision = self.next_revision(expected)?;
                let Some(PendingWork::LandPlay(work)) = &self.pending_work else {
                    return Err(EffectError::WrongResolutionFrame);
                };
                if self.land_play_entry_frame(work)?.as_ref() != Some(bound) {
                    return Err(EffectError::WrongResolutionFrame);
                }
                if actor != bound.chooser {
                    return Err(EffectError::WrongChooser);
                }
                let DecisionReply::EntryChoice(choice) = reply else {
                    return Err(EffectError::WrongDecisionKind);
                };
                if !self.valid_entry_choice(bound, choice) {
                    return Err(EffectError::IllegalTarget);
                }
                let mut next = self.clone();
                next.revision = revision;
                let Some(PendingWork::LandPlay(work)) = &mut next.pending_work else {
                    return Err(EffectError::WrongResolutionFrame);
                };
                work.choice = Some(choice);
                next.progress_land_play()
            }
            EffectDecision::EntryChoice(bound) => {
                let revision = self.next_revision(expected)?;
                let linked = self.linked_return_work.is_some();
                if linked {
                    if self.linked_return_decision()?.as_ref() != Some(frame) {
                        return Err(EffectError::WrongResolutionFrame);
                    }
                } else if self.entry_choice_frame()?.as_ref() != Some(bound) {
                    return Err(EffectError::WrongResolutionFrame);
                }
                if actor != bound.chooser {
                    return Err(EffectError::WrongChooser);
                }
                let DecisionReply::EntryChoice(choice) = reply else {
                    return Err(EffectError::WrongDecisionKind);
                };
                if !self.valid_entry_choice(bound, choice) {
                    return Err(EffectError::IllegalTarget);
                }
                let mut next = self.clone();
                next.revision = revision;
                if linked {
                    next.linked_return_work
                        .as_mut()
                        .ok_or(EffectError::WrongResolutionFrame)?
                        .choices
                        .insert(bound.object, choice);
                    return next.continue_linked_returns(turn);
                }
                next.resolution_entry_choices.insert(bound.object, choice);
                if next.resolution_cursor().is_none() {
                    let entry = next
                        .window
                        .resolving()
                        .ok_or(EffectError::WrongPhase)?
                        .clone();
                    return next.resolve_permanent_entry(&entry);
                }
                next.progress_rules(
                    DamageJournal::default(),
                    vec![],
                    vec![],
                    DrawJournal::default(),
                    RulesJournal::default(),
                )
            }
            EffectDecision::ZoneReplacement(frame)
                if matches!(self.pending_work, Some(PendingWork::Checks(_))) =>
            {
                self.resume_check_replacement(expected, frame, actor, reply, turn)
            }
            EffectDecision::LibraryTop(_) => self.resume_library_top(expected, frame, actor, reply),
            EffectDecision::ResolutionGraveyardOrder(_)
                if matches!(self.pending_work, Some(PendingWork::LibraryTop(_))) =>
            {
                self.resume_library_top(expected, frame, actor, reply)
            }
            EffectDecision::ZoneReplacement(_)
                if matches!(self.pending_work, Some(PendingWork::LibraryTop(_))) =>
            {
                self.resume_library_top(expected, frame, actor, reply)
            }
            EffectDecision::ZoneReplacement(_)
            | EffectDecision::ResolutionSelect(_)
            | EffectDecision::ResolutionGraveyardOrder(_) => {
                self.resume_resolution_selection(expected, frame, actor, reply)
            }
            EffectDecision::OptionalEffect(bound)
                if self.pending_life_replacement_decision().as_ref() == Some(frame) =>
            {
                self.resume_life_replacement(expected, bound, actor, reply)
            }
            EffectDecision::OptionalEffect(_)
                if matches!(self.pending_work, Some(PendingWork::LibraryTop(_))) =>
            {
                self.resume_library_top(expected, frame, actor, reply)
            }
            EffectDecision::OptionalEffect(bound) => {
                self.resume_optional_effect(expected, bound, actor, reply, turn)
            }
            EffectDecision::ReplacementChooser(bound) => {
                self.resume_replacement_chooser(expected, bound, actor, reply)
            }
            EffectDecision::ResolutionPayment(bound) => {
                self.resume_resolution_payment(expected, bound, actor, reply, turn)
            }
            EffectDecision::TokenDefender(bound) => {
                let DecisionReply::TokenDefender(defender) = reply else {
                    return Err(EffectError::WrongDecisionKind);
                };
                self.accept_token_defender(expected, bound, actor, defender)?
                    .progress_rules(
                        DamageJournal::default(),
                        vec![],
                        vec![],
                        DrawJournal::default(),
                        RulesJournal::default(),
                    )
            }
            EffectDecision::TransformOrder(bound) => {
                let DecisionReply::TransformOrder(order) = reply else {
                    return Err(EffectError::WrongDecisionKind);
                };
                let revision = self.next_revision(expected)?;
                if actor != bound.chooser() {
                    return Err(EffectError::WrongChooser);
                }
                let mut next = self.clone();
                next.choose_resolution_transform_order(bound, order)?;
                next.revision = revision;
                next.progress_rules(
                    DamageJournal::default(),
                    vec![],
                    vec![],
                    DrawJournal::default(),
                    RulesJournal::default(),
                )
            }
            EffectDecision::UntapKeepTapped(_) => Err(EffectError::WrongPhase),
            EffectDecision::TimestampOrder(bound) => {
                let DecisionReply::TimestampOrder(order) = reply else {
                    return Err(EffectError::WrongDecisionKind);
                };
                let next = self.accept_token_timestamp_order(expected, bound, actor, order)?;
                next.progress_rules(
                    DamageJournal::default(),
                    vec![],
                    vec![],
                    DrawJournal::default(),
                    RulesJournal::default(),
                )
            }
            EffectDecision::CounterRemoval(bound) => {
                let DecisionReply::RemoveCounters(selected) = reply else {
                    return Err(EffectError::WrongDecisionKind);
                };
                let revision = self.next_revision(expected)?;
                if self.counter_removal_choice()? != *bound {
                    return Err(EffectError::WrongResolutionFrame);
                }
                if actor != bound.chooser() {
                    return Err(EffectError::WrongChooser);
                }
                let move_to =
                    if let Ok(Instruction::MoveCounters { to, .. }) = self.current_instruction() {
                        let entry = self
                            .window
                            .resolving()
                            .ok_or(EffectError::WrongResolutionFrame)?;
                        self.resolve_pass_c_move_subject(entry, *to)?
                    } else {
                        None
                    };
                let mut next = self.clone();
                next.revision = revision;
                let mut append = RulesJournal::default();
                next.remove_counters(bound, &selected, &mut append)?;
                if let Some(dest) = move_to
                    && dest != bound.object()
                    && next
                        .window
                        .cards()
                        .get(dest)
                        .is_some_and(|c| c.zone() == ObjectZone::Battlefield)
                {
                    let moved: BTreeMap<CounterKind, RulesInteger> = selected
                        .iter()
                        .filter(|(_, count)| **count > 0)
                        .map(|(&kind, &count)| (kind, RulesInteger::from(i64::from(count))))
                        .collect();
                    next.place_moved_counters(
                        bound.entry(),
                        bound.instruction(),
                        dest,
                        &moved,
                        &mut append,
                    )?;
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
            EffectDecision::Proliferate(bound) => {
                let DecisionReply::Proliferate(selected) = reply else {
                    return Err(EffectError::WrongDecisionKind);
                };
                let revision = self.next_revision(expected)?;
                if self.proliferate_choice()? != *bound {
                    return Err(EffectError::WrongResolutionFrame);
                }
                if actor != bound.chooser() {
                    return Err(EffectError::WrongChooser);
                }
                let mut next = self.clone();
                next.revision = revision;
                let mut append = RulesJournal::default();
                next.proliferate(bound, selected, &mut append)?;
                next.progress_rules(
                    DamageJournal::default(),
                    vec![],
                    vec![],
                    DrawJournal::default(),
                    append,
                )
            }
            EffectDecision::CreatureType(bound) => {
                self.ensure_resolution_or_idle()?;
                let DecisionReply::CreatureType(selected) = reply else {
                    return Err(EffectError::WrongDecisionKind);
                };
                let accepted = self.accept_creature_type(expected, bound, actor, selected)?;
                let (next, (), append) = accepted.into_parts();
                next.progress_rules(
                    DamageJournal::default(),
                    vec![],
                    vec![],
                    DrawJournal::default(),
                    append,
                )
            }
            EffectDecision::Checks(checks) => {
                self.resume_checks(expected, checks, actor, reply, turn)
            }
            EffectDecision::Triggers(order) => {
                self.resume_trigger_order(expected, order, actor, reply, turn)
            }
            EffectDecision::TriggerTarget(frame) => {
                self.resume_trigger_target(expected, frame, actor, reply, turn)
            }
            EffectDecision::Combat(combat) => {
                let turn = turn.ok_or(EffectError::WrongPhase)?;
                match (combat, reply) {
                    (
                        CombatDecision::DamageAssignments(bound),
                        DecisionReply::AssignCombatDamage(rows),
                    ) => self.resume_combat_damage_assignments(expected, bound, actor, rows, turn),
                    (CombatDecision::DamageAssignments(_), _) => {
                        Err(EffectError::WrongDecisionKind)
                    }
                    (_, other) => self.resume_combat(expected, combat, actor, other, turn),
                }
            }
            EffectDecision::Damage(damage) => {
                self.resume_damage(expected, damage, actor, reply, turn)
            }
            EffectDecision::CopyTargets(bound) => {
                self.ensure_resolution_or_idle()?;
                let DecisionReply::CopyTargets(choice) = reply else {
                    return Err(EffectError::WrongDecisionKind);
                };
                let revision = self.next_revision(expected)?;
                if &self.copy_resolution_frame()? != bound {
                    return Err(EffectError::WrongResolutionFrame);
                }
                if actor != bound.chooser {
                    return Err(EffectError::WrongChooser);
                }
                let entry = self
                    .window
                    .resolving()
                    .ok_or(EffectError::WrongPhase)?
                    .clone();
                let mut next = self.clone();
                next.revision = revision;
                let mut journal = DamageJournal::default();
                let mut append = RulesJournal::default();
                if matches!(
                    next.current_instruction()?,
                    Instruction::CopyForOtherCreatures
                        | Instruction::CopyResolvingSpell
                        | Instruction::CopyOnCast(_)
                ) {
                    next.add_copy_choice(entry.id(), actor, choice)?;
                    return next.progress_rules(
                        journal,
                        vec![],
                        vec![],
                        DrawJournal::default(),
                        append,
                    );
                }
                let instruction = next.current_instruction()?.clone();
                if instruction == Instruction::CopyCapturedAbility {
                    let source = next.delayed_ability_source(entry.id())?;
                    if let Some(changes) =
                        next.collect_delayed_target(entry.id(), &source, choice)?
                    {
                        next.finish_delayed_ability_copy(&entry, &source, changes, &mut append)?;
                    }
                    return next.progress_rules(
                        journal,
                        vec![],
                        vec![],
                        DrawJournal::default(),
                        append,
                    );
                }
                let selected = if instruction == Instruction::CopyAbility {
                    let source =
                        next.capture_ability(bound.original, &next.evaluated_characteristics()?)?;
                    next.collect_delayed_target(entry.id(), &source, choice)?
                } else if matches!(
                    instruction,
                    Instruction::ChooseNewTargets | Instruction::ChangeOnlyTarget
                ) {
                    next.collect_existing_target(
                        entry.id(),
                        bound.original,
                        choice,
                        instruction == Instruction::ChangeOnlyTarget,
                    )?
                } else {
                    let source = next.capture_copy_source(bound.original, 0.into())?;
                    next.collect_copy_target(entry.id(), &source, actor, choice)?
                };
                let Some(changes) = selected else {
                    return next.progress_rules(
                        journal,
                        vec![],
                        vec![],
                        DrawJournal::default(),
                        append,
                    );
                };
                next.apply_instruction(
                    entry,
                    instruction,
                    Some((actor, changes)),
                    &mut journal,
                    &mut append,
                )?;
                next.advance_instruction()?;
                next.progress_rules(journal, vec![], vec![], DrawJournal::default(), append)
            }
        }
    }
}
