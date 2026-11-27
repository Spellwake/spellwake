//! Actual calendar entry, ended-boundary resources and combat occurrence capture.
use super::*;
use crate::{
    effects::occurrence_flow::status_occurrences,
    occurrence::{EventCause, RulesOccurrence, StatusReason},
    turn::{Step, TurnStage},
};

impl EffectWindow {
    fn lifecycle_player_relation_matches(
        &self,
        rel: crate::program::PlayerRelation,
        rule_ctrl: SeatId,
        candidate: SeatId,
    ) -> Result<bool, EffectError> {
        Ok(match rel {
            crate::program::PlayerRelation::Controller => candidate == rule_ctrl,
            crate::program::PlayerRelation::Opponent => self
                .combat_rules
                .opponents(self.window.participants(), rule_ctrl)?
                .contains(&candidate),
            crate::program::PlayerRelation::AnyParticipant => true,
        })
    }

    pub(super) fn retained_mana_types(
        &self,
        current: &EvaluatedCharacteristics,
    ) -> BTreeSet<(SeatId, ManaType)> {
        let mut retained = BTreeSet::new();
        for active in &self.temporary_rules {
            if let Some(ability) = self.active_temporary_static(active, current)
                && let crate::characteristics::StaticContinuousAbility::Modifier {
                    rule: crate::characteristics::StaticModifierRule::RetainMana(kind),
                    ..
                } = ability
            {
                retained.insert((active.controller, *kind));
            }
        }
        for object in current
            .objects
            .values()
            .filter(|p| p.zone == ObjectZone::Battlefield)
        {
            let Some(player) = object.controller else {
                continue;
            };
            for ability in object.active_battlefield_rules() {
                if let crate::characteristics::StaticContinuousAbility::Modifier {
                    rule: crate::characteristics::StaticModifierRule::RetainMana(kind),
                    ..
                } = ability
                {
                    retained.insert((player, *kind));
                }
            }
        }
        retained
    }

    pub(super) fn converted_unspent_mana_types(
        &self,
        current: &EvaluatedCharacteristics,
    ) -> BTreeMap<SeatId, ManaType> {
        let mut converted = BTreeMap::new();
        for active in &self.temporary_rules {
            if let Some(ability) = self.active_temporary_static(active, current)
                && let crate::characteristics::StaticContinuousAbility::Modifier {
                    rule: crate::characteristics::StaticModifierRule::ConvertUnspentMana(target),
                    ..
                } = ability
            {
                converted.insert(active.controller, *target);
            }
        }
        for object in current
            .objects
            .values()
            .filter(|p| p.zone == ObjectZone::Battlefield)
        {
            let Some(player) = object.controller else {
                continue;
            };
            for ability in object.active_battlefield_rules() {
                if let crate::characteristics::StaticContinuousAbility::Modifier {
                    rule: crate::characteristics::StaticModifierRule::ConvertUnspentMana(target),
                    ..
                } = ability
                {
                    converted.insert(player, *target);
                }
            }
        }
        converted
    }

    /// Whether active battlefield or temporary rules (not one-shot step skips) cause `active` to skip `step`.
    pub(crate) fn has_static_skip_step(
        &self,
        active: SeatId,
        step: Step,
    ) -> Result<bool, EffectError> {
        let check_ability = |ability: &crate::characteristics::StaticContinuousAbility,
                             rule_ctrl: SeatId|
         -> Result<bool, EffectError> {
            match ability {
                crate::characteristics::StaticContinuousAbility::Restriction {
                    rule:
                        crate::characteristics::StaticRestriction::SkipStep {
                            player,
                            step: skipped,
                        },
                    ..
                } if *skipped == step => {
                    self.lifecycle_player_relation_matches(*player, rule_ctrl, active)
                }
                crate::characteristics::StaticContinuousAbility::DrawRule {
                    rule: crate::characteristics::StaticDrawRule::SkipControllerDrawStep,
                    ..
                } if step == Step::Draw => Ok(rule_ctrl == active),
                _ => Ok(false),
            }
        };
        let evaluated = self.evaluated_characteristics()?;
        for entry in &self.temporary_rules {
            if let Some(ability) = self.active_temporary_static(entry, &evaluated)
                && check_ability(ability, entry.controller)?
            {
                return Ok(true);
            }
        }
        for object in evaluated
            .objects
            .values()
            .filter(|p| p.zone == ObjectZone::Battlefield)
        {
            let Some(rule_ctrl) = object.controller else {
                continue;
            };
            for ability in object.active_battlefield_rules() {
                if check_ability(ability, rule_ctrl)? {
                    return Ok(true);
                }
            }
            for rule in &object.granted_restrictions {
                if let crate::characteristics::StaticRestriction::SkipStep {
                    player,
                    step: skipped,
                } = rule
                    && *skipped == step
                    && self.lifecycle_player_relation_matches(*player, rule_ctrl, active)?
                {
                    return Ok(true);
                }
            }
        }
        Ok(false)
    }

    /// Whether active static rules or a pending one-shot skip effect causes `active` to skip `step`.
    pub(crate) fn should_skip_step(&self, active: SeatId, step: Step) -> Result<bool, EffectError> {
        if self.has_static_skip_step(active, step)? {
            return Ok(true);
        }
        Ok(self
            .skip_steps_remaining
            .get(&(active, step))
            .copied()
            .unwrap_or(0)
            > 0)
    }

    /// Consume one pending one-shot skip for `(active, step)` if no static rule is already skipping it (CR 614.10).
    pub(crate) fn consume_skipped_step(
        &mut self,
        active: SeatId,
        step: Step,
    ) -> Result<bool, EffectError> {
        if self.has_static_skip_step(active, step)? {
            return Ok(false);
        }
        if let Some(remaining) = self.skip_steps_remaining.get_mut(&(active, step))
            && *remaining > 0
        {
            *remaining -= 1;
            if *remaining == 0 {
                self.skip_steps_remaining.remove(&(active, step));
            }
            return Ok(true);
        }
        Ok(false)
    }

    /// Net signed delta applied to `player`'s maximum hand size from all active
    /// `StaticModifierRule::ModifyMaxHandSize` rules (CR 402.2).
    ///
    /// # Errors
    /// Propagates characteristic or opponent-relation evaluation failures.
    pub fn max_hand_size_delta(&self, player: SeatId) -> Result<i32, EffectError> {
        let mut total = 0i32;
        let evaluated = self.evaluated_characteristics()?;
        for entry in &self.temporary_rules {
            if let Some(ability) = self.active_temporary_static(entry, &evaluated)
                && let crate::characteristics::StaticContinuousAbility::Modifier {
                    rule:
                        crate::characteristics::StaticModifierRule::ModifyMaxHandSize {
                            player: rel,
                            delta,
                        },
                    ..
                } = ability
                && self.lifecycle_player_relation_matches(*rel, entry.controller, player)?
            {
                total = total.saturating_add(i32::from(*delta));
            }
        }
        for object in evaluated
            .objects
            .values()
            .filter(|p| p.zone == ObjectZone::Battlefield)
        {
            let Some(rule_ctrl) = object.controller else {
                continue;
            };
            for ability in object.active_battlefield_rules() {
                if let crate::characteristics::StaticContinuousAbility::Modifier {
                    rule:
                        crate::characteristics::StaticModifierRule::ModifyMaxHandSize {
                            player: rel,
                            delta,
                        },
                    ..
                } = ability
                    && self.lifecycle_player_relation_matches(*rel, rule_ctrl, player)?
                {
                    total = total.saturating_add(i32::from(*delta));
                }
            }
        }
        Ok(total)
    }

    pub(crate) fn take_additional_combat(&mut self) -> bool {
        if self.additional_combats == 0 {
            return false;
        }
        self.additional_combats -= 1;
        true
    }

    fn consume_skip_turn(&mut self, seat: SeatId) -> bool {
        if let Some(remaining) = self.skip_turns_remaining.get_mut(&seat)
            && *remaining > 0
        {
            *remaining -= 1;
            if *remaining == 0 {
                self.skip_turns_remaining.remove(&seat);
            }
            return true;
        }
        false
    }

    /// Choose the next actual turn and remember departed seats passed along the way.
    /// Most recently created extra turns precede ordinary rotation (CR 500.7).
    /// Skipped turns for living players (CR 614.10 / CR 614.10a) are consumed without
    /// entering `skipped_turns`, so "until your next turn" effects and phasing do not trigger.
    pub(crate) fn take_next_active_player(&mut self) -> Result<SeatId, EffectError> {
        let table = self.window.participants().to_vec();
        if table.is_empty() {
            return Err(EffectError::WrongPhase);
        }
        let current = self.window.active_player();
        let ordinary = match self.resume_turn_order_at {
            Some(resume) => resume,
            None => self
                .window
                .original_turn_order_from(current)?
                .nth(1)
                .ok_or(EffectError::WrongPhase)?,
        };
        self.skipped_turns.clear();
        while let Some(extra) = self.extra_turns.pop() {
            if !table.contains(&extra) {
                self.skipped_turns.insert(extra);
                continue;
            }
            if self.consume_skip_turn(extra) {
                continue;
            }
            self.resume_turn_order_at = Some(ordinary);
            return Ok(extra);
        }
        self.resume_turn_order_at = None;
        let cycle: Vec<SeatId> = self.window.original_turn_order_from(ordinary)?.collect();
        if cycle.is_empty() {
            return Err(EffectError::WrongPhase);
        }
        loop {
            for &seat in &cycle {
                if !table.contains(&seat) {
                    self.skipped_turns.insert(seat);
                    continue;
                }
                if self.consume_skip_turn(seat) {
                    continue;
                }
                return Ok(seat);
            }
        }
    }

    /// Adopt one actual scheduler result. Publication does not add owner revisions.
    /// Prepared construction deliberately uses raw enter_turn_step instead.
    pub(crate) fn enter_calendar(
        &self,
        expected: u64,
        previous: Option<&TurnScheduler>,
        next_turn: &TurnScheduler,
        active: SeatId,
    ) -> Result<RulesTransition<Self, ()>, EffectError> {
        if expected != self.revision {
            return Err(StackWindowError::StaleRevision.into());
        }
        if previous.is_none() && self.window.phase() != WindowPhase::Opening {
            return Err(EffectError::WrongPhase);
        }
        let position = next_turn.position();
        let entered = previous.is_none_or(|turn| turn.position() != position);
        let starts_turn = previous.is_none_or(|turn| turn.position().number != position.number);
        let mut next = self.clone();
        let mut append = RulesJournal::default();
        if entered {
            if let Some(ended) = previous
                && ended.position().step == Step::End
            {
                let ended_active = self.window.active_player();
                let mut updated_permissions = BTreeSet::new();
                for mut grant in std::mem::take(&mut next.card_play_permissions) {
                    match grant.duration {
                        super::CardPlayDuration::UntilEndOfNextEndStep(owner)
                            if owner == ended_active => {}
                        super::CardPlayDuration::UntilEndOfNextEndStepSkipCurrent(owner)
                            if owner == ended_active =>
                        {
                            grant.duration = super::CardPlayDuration::UntilEndOfNextEndStep(owner);
                            updated_permissions.insert(grant);
                        }
                        _ => {
                            updated_permissions.insert(grant);
                        }
                    }
                }
                next.card_play_permissions = updated_permissions;
            }
            if position.step == Step::End {
                next.card_play_permissions.retain(|grant| {
                    !matches!(
                        grant.duration,
                        super::CardPlayDuration::UntilNextEndStep(owner) if owner == active
                    )
                });
            }
            if position.step == Step::Upkeep {
                next.card_play_permissions.retain(|grant| {
                    !matches!(
                        grant.duration,
                        super::CardPlayDuration::UntilNextUpkeep(owner) if owner == active
                    )
                });
            }
        }
        if let Some(ended) = previous.filter(|_| entered && !self.mana.is_empty()) {
            let before = self.rules_snapshot(Some(ended))?;
            let retained = self.retained_mana_types(&before.view().evaluated);
            let converted = self.converted_unspent_mana_types(&before.view().evaluated);
            if ended.position().step == crate::turn::Step::EndCombat {
                next.floating_mana.end_combat_effects();
            }
            next.expire_step_mana_with_conversion(&retained, &converted);
            let mut values = Vec::new();
            for player in self.window.apnap_players()? {
                if converted.contains_key(&player) {
                    continue;
                }
                let mut expired = BTreeMap::new();
                let mut remaining = BTreeMap::new();
                for kind in ManaType::ALL {
                    let kept = next.mana(player, kind);
                    let lost = self.mana(player, kind).minus(&kept);
                    if kept.is_positive() {
                        remaining.insert(kind, kept);
                    }
                    if lost.is_positive() {
                        expired.insert(kind, lost);
                    }
                }
                if expired.is_empty() {
                    continue;
                }
                values.push(if remaining.is_empty() {
                    RulesOccurrence::ManaPoolEmptied {
                        player,
                        previous: expired,
                    }
                } else {
                    RulesOccurrence::ManaExpired {
                        player,
                        expired,
                        remaining,
                    }
                });
            }
            if !values.is_empty() {
                next.emit_from(
                    EventCause::ManaPoolsEmptied {
                        ended: ended.position(),
                    },
                    &before,
                    Some(ended),
                    &mut append,
                    |_, _, _| Ok(values),
                )?;
            }
        }
        let (state, (), records) = next
            .enter_turn_step(
                next.revision(),
                active,
                next_turn.stage() == TurnStage::Priority,
                false,
            )?
            .into_parts();
        next = state;
        next.set_calendar_position(position);
        append.append(records);
        if starts_turn {
            if !next.skipped_turns.is_empty() {
                let before = next.rules_snapshot(Some(next_turn))?;
                let expired_effects = next
                    .characteristics
                    .expire_at_skipped_turns(&next.skipped_turns);
                let players = next.skipped_turns.clone();
                next.emit_from(
                    EventCause::TurnOrderAdvanced { position },
                    &before,
                    Some(next_turn),
                    &mut append,
                    |_, _, _| {
                        Ok(vec![RulesOccurrence::TurnsSkipped {
                            players,
                            expired_effects,
                        }])
                    },
                )?;
            }
            next.must_block_this_turn.clear();
            next.must_be_blocked_this_turn.clear();
            if let Some(cards) = next.recent_graveyard_cards.as_mut() {
                cards.clear();
            }
            next.combat_damaged_players_by_source.clear();
            next.activated_this_turn.clear();
            next.ability_resolutions_this_turn.clear();
            next.chosen_modal_modes_turn.clear();
            next.source_cast_uses.clear();
            next.characteristics
                .expire_at_turn_start(active, position.number);
            next.temporary_rules
                .retain(|rule| !rule.duration.expires_at_turn_start(active, position.number));
            next.triggers
                .expire_delayed_event_triggers_at_turn_start(active, position.number);
            next.permanent_goaded.retain(|id, players| {
                !players.is_empty()
                    && next.window.cards().get(*id).is_some_and(|c| {
                        c.zone() == crate::object_identity::ObjectZone::Battlefield
                    })
            });
            next.goaded.retain(|id, players| {
                let permanent = next.permanent_goaded.get(id);
                players.retain(|player| {
                    permanent.is_some_and(|perm| perm.contains(player))
                        || (*player != active && !next.skipped_turns.contains(player))
                });
                !players.is_empty()
                    && next.window.cards().get(*id).is_some_and(|c| {
                        c.zone() == crate::object_identity::ObjectZone::Battlefield
                    })
            });
            let casts = std::mem::take(&mut next.spells_cast_by_player);
            next.previous_active_spells = previous.map(|_| {
                casts
                    .get(&self.window.active_player())
                    .copied()
                    .unwrap_or_default()
            });
            next.previous_turn_casts = previous.and_then(|_| {
                use crate::program::PreviousTurnCastCondition::*;
                if casts.is_empty() {
                    Some(NoSpells)
                } else if casts.values().any(|count| *count >= 2) {
                    Some(OnePlayerCastAtLeastTwo)
                } else {
                    None
                }
            });
            next.turn_start_incarnation = next.window.cards().next_incarnation();
            next.enter_speed_turn(position.number, active);
            if let Some(damage) = &mut next.damage_to_players_this_turn {
                damage.clear();
            }
            if let Some(history) = &mut next.cast_history {
                *history = CastTurnHistory::default();
            }
            let prev_life_lost = if previous.is_some() {
                next.characteristics.life_lost_this_turn.clone()
            } else {
                BTreeMap::new()
            };
            next = next.record_turn_start(next.revision())?;
            next.characteristics.life_lost_last_turn = prev_life_lost;
            next.characteristics.cast_spells_this_turn.clear();
            next.characteristics
                .activated_graveyard_ability_this_turn
                .clear();
            next.characteristics.cards_exiled_this_turn = 0;
            next.characteristics
                .cards_put_into_graveyard_this_turn
                .clear();
            next.characteristics.sacrificed_this_turn.clear();
            next.characteristics.damage_dealt_this_turn_by.clear();
            next.characteristics
                .damage_dealt_to_opponents_this_turn
                .clear();
            next.characteristics
                .combat_damage_dealt_to_player_this_turn
                .clear();
            next.characteristics
                .excess_damage_received_this_turn
                .clear();
        }
        if entered {
            // The new active/window and actual control-history stamp already exist.
            let before = next.rules_snapshot(Some(next_turn))?;
            if position.step == Step::BeginningCombat {
                next.characteristics.combat_phases_this_turn = next
                    .characteristics
                    .combat_phases_this_turn
                    .saturating_add(1);
            }
            if position.step == Step::PrecombatMain {
                let current = next.evaluated_characteristics()?;
                for (&id, permanent) in &current.objects {
                    if permanent.zone == ObjectZone::Battlefield
                        && permanent.controller == Some(active)
                        && permanent.final_saga_chapter().is_some()
                    {
                        let count = next
                            .characteristics
                            .lore
                            .entry(id)
                            .or_insert_with(|| 0.into());
                        *count = count.plus(&1.into());
                    }
                }
            }
            next.emit_from(
                EventCause::StepBegan { position },
                &before,
                Some(next_turn),
                &mut append,
                |_, _, _| Ok(vec![RulesOccurrence::StepBegan { position, active }]),
            )?;
            if position.step == Step::Upkeep
                && let Some(cuts) = &mut next.characteristics.upkeep_cuts
            {
                cuts.insert(active, next.window.cards().control_cut());
            }
        }
        Ok(RulesTransition::new(next, (), append))
    }

    /// End the actual old combat phase before pool loss or the next step entry.
    pub(crate) fn finish_combat_on_turn(
        &self,
        expected: u64,
        turn: &TurnScheduler,
    ) -> Result<RulesTransition<Self, ()>, EffectError> {
        if expected != self.revision {
            return Err(StackWindowError::StaleRevision.into());
        }
        if turn.position().step.phase() != crate::turn::Phase::Combat
            || self.window.phase() != WindowPhase::StepEnded
            || self.pending_work.is_some()
        {
            return Err(EffectError::WrongPhase);
        }
        if !self.state_based_losses.is_empty() {
            return Err(EffectError::PlayerLossPending);
        }
        let before = self.rules_snapshot(Some(turn))?;
        let previous = self.combat.clone();
        let mut append = RulesJournal::default();
        let mut next = if previous.is_some() {
            let (state, (), records) = self.finish_combat(expected)?.into_parts();
            append.append(records);
            state
        } else {
            // The old root skipped the physical clear when no context existed.
            self.clone()
        };
        next.characteristics.expire_at_end_of_combat();
        next.temporary_rules
            .retain(|rule| !rule.duration.expires_at_end_of_combat());
        next.triggers
            .expire_delayed_event_triggers_at_end_of_combat();
        next.emit_from(
            EventCause::CombatEnded {
                position: turn.position(),
            },
            &before,
            Some(turn),
            &mut append,
            |_, _, _| Ok(vec![RulesOccurrence::CombatEnded { previous }]),
        )?;
        Ok(RulesTransition::new(next, (), append))
    }

    /// Direct physical-owner unit convenience, without a root calendar context.
    #[cfg(test)]
    pub(crate) fn untap_turn(
        &self,
        expected: u64,
    ) -> Result<RulesTransition<Self, ()>, EffectError> {
        let (state, _, journal) = self.untap_with_context(expected, None)?.into_parts();
        Ok(RulesTransition::new(state, (), journal))
    }
    #[cfg(test)]
    pub(crate) fn untap_on_turn(
        &self,
        expected: u64,
        turn: &TurnScheduler,
    ) -> Result<RulesTransition<Self, ()>, EffectError> {
        if turn.position().step != Step::Untap || turn.stage() != TurnStage::Work(TurnWork::Untap) {
            return Err(EffectError::WrongPhase);
        }
        let (state, _, journal) = self.untap_with_context(expected, Some(turn))?.into_parts();
        Ok(RulesTransition::new(state, (), journal))
    }
    pub(crate) fn untap_on_turn_with_choice(
        &self,
        expected: u64,
        turn: &TurnScheduler,
    ) -> Result<RulesTransition<Self, Option<UntapKeepTappedFrame>>, EffectError> {
        if turn.position().step != Step::Untap || turn.stage() != TurnStage::Work(TurnWork::Untap) {
            return Err(EffectError::WrongPhase);
        }
        self.untap_with_context(expected, Some(turn))
    }
    pub(crate) fn choose_untap_keep_tapped(
        &mut self,
        frame: &UntapKeepTappedFrame,
        objects: Vec<ObjectId>,
    ) -> Result<(), EffectError> {
        if frame.revision != self.revision || frame.chooser != self.window.active_player() {
            return Err(EffectError::WrongDecisionKind);
        }
        let chosen: BTreeSet<ObjectId> = objects.iter().copied().collect();
        if chosen.len() != objects.len() || !chosen.iter().all(|id| frame.objects.contains(id)) {
            return Err(EffectError::IllegalTarget);
        }
        self.untap_keep_tapped = Some(chosen);
        Ok(())
    }
    fn untap_with_context(
        &self,
        expected: u64,
        turn: Option<&TurnScheduler>,
    ) -> Result<RulesTransition<Self, Option<UntapKeepTappedFrame>>, EffectError> {
        let revision = self.next_revision(expected)?;
        if self.pending_work.is_some() {
            return Err(EffectError::WrongPhase);
        }
        if !self.state_based_losses.is_empty() {
            return Err(EffectError::PlayerLossPending);
        }
        let mut next = self.clone();
        next.revision = revision;
        let mut append = RulesJournal::default();
        let active = next.window.active_player();
        if next.should_skip_step(active, Step::Untap)? {
            next.consume_skipped_step(active, Step::Untap)?;
            return Ok(RulesTransition::new(next, None, append));
        }
        if !next.untap_preface_done {
            let before_phasing = next.rules_snapshot(turn)?;
            let roots = before_phasing
                .view()
                .evaluated
                .objects
                .values()
                .filter(|object| {
                    object.zone == ObjectZone::Battlefield
                        && object.controller == Some(active)
                        && object.has_keyword(KeywordAbility::Phasing)
                })
                .map(|object| object.object)
                .collect::<Vec<_>>();
            let (departed, returned) = next.window.phase_at_untap(
                &roots,
                &std::mem::take(&mut next.skipped_turns),
                &next.characteristics.attachments,
            )?;
            let changes = [(true, departed), (false, returned)]
                .into_iter()
                .filter(|(_, objects)| !objects.is_empty())
                .map(|(out, objects)| RulesOccurrence::Phased { objects, out })
                .collect::<Vec<_>>();
            if !changes.is_empty() {
                next.emit_from(
                    EventCause::UntapTurn { active },
                    &before_phasing,
                    turn,
                    &mut append,
                    |_, _, _| Ok(changes),
                )?;
            }

            let previous = next.day_night();
            if let Some(count) = next.previous_active_spells.take()
                && turn.is_some()
                && let Some(clock) = previous.untap(count)
            {
                let before = next.rules_snapshot(turn)?;
                let (_, faces) =
                    next.reconcile_day_night(Some(clock), &next.evaluated_characteristics()?)?;
                next.transform_orders.clear();
                next.emit_from(
                    EventCause::UntapTurn {
                        active: next.window.active_player(),
                    },
                    &before,
                    turn,
                    &mut append,
                    |old, new, _| Self::day_change_occurrences(previous, clock, &faces, old, new),
                )?;
            }
            next.untap_preface_done = true;
        }
        let before = next.rules_snapshot(turn)?;
        let evaluated = next.evaluated_characteristics()?;
        let mut excluded = BTreeSet::new();
        for source in evaluated.objects.values() {
            if source.zone == ObjectZone::Battlefield
                && source.controller == Some(active)
                && (source.cannot_be_untapped || source.doesnt_untap_during_untap_step)
            {
                excluded.insert(source.object);
            }
            if source.controller == Some(active)
                && source.has_battlefield_rule(|rule| {
                    matches!(
                        rule,
                        crate::characteristics::StaticContinuousAbility::UntapRule {
                            rule: crate::characteristics::StaticUntapRule::SourceSkipsUntap,
                            ..
                        }
                    )
                })
            {
                excluded.insert(source.object);
            }
            let always = source.has_battlefield_rule(|rule| {
                matches!(
                    rule,
                    crate::characteristics::StaticContinuousAbility::UntapRule {
                        rule: crate::characteristics::StaticUntapRule::EnchantedCreatureSkipsUntap,
                        ..
                    }
                )
            });
            if !always && !source.has_battlefield_rule(|rule| matches!(
                rule, crate::characteristics::StaticContinuousAbility::UntapRule { rule: crate::characteristics::StaticUntapRule::EnchantedCreatureUntapsOnlyForMonarch, .. }
            )) {
                continue;
            }
            let Some(TargetRef::Object(host)) = source.attached_to else {
                continue;
            };
            if evaluated.get(host).is_some_and(|object| {
                object.zone == ObjectZone::Battlefield
                    && object.is_creature()
                    && object.controller == Some(next.window.active_player())
                    && (always || object.controller != next.characteristics.monarch)
            }) {
                excluded.insert(host);
            }
        }
        excluded.extend(
            next.untap_locks
                .iter()
                .filter_map(|&(target, source, epoch)| {
                    (evaluated
                        .get(target)
                        .is_some_and(|card| card.controller == Some(active))
                        && next
                            .window
                            .cards()
                            .get(source)
                            .is_some_and(|card| card.control_epoch() == Some(epoch)))
                    .then_some(target)
                }),
        );
        // Check CR 502.3 "you may choose not to untap" before consuming skip_next_controller_untap or stun counters.
        if turn.is_some() && next.untap_keep_tapped.is_none() {
            let mut optional_keep_tapped: Vec<ObjectId> = evaluated
                .objects
                .values()
                .filter(|source| {
                    source.zone == ObjectZone::Battlefield
                        && source.controller == Some(active)
                        && !excluded.contains(&source.object)
                        && !next.skip_next_controller_untap.contains(&source.object)
                        && next
                            .window
                            .cards()
                            .get(source.object)
                            .and_then(GameObject::tapped)
                            == Some(true)
                        && source.has_battlefield_rule(|rule| {
                            matches!(
                                rule,
                                crate::characteristics::StaticContinuousAbility::UntapRule {
                                    rule: crate::characteristics::StaticUntapRule::MayChooseNotToUntapSource,
                                    ..
                                }
                            )
                        })
                })
                .map(|source| source.object)
                .collect();
            optional_keep_tapped.sort();
            if !optional_keep_tapped.is_empty() {
                let frame = UntapKeepTappedFrame {
                    revision: next.revision,
                    chooser: active,
                    objects: optional_keep_tapped,
                };
                return Ok(RulesTransition::new(next, Some(frame), append));
            }
        }
        if let Some(chosen_keep) = next.untap_keep_tapped.take() {
            excluded.extend(chosen_keep);
        }
        next.untap_preface_done = false;
        next.skip_next_controller_untap.retain(|id| {
            let Some(card) = next
                .window
                .cards()
                .get(*id)
                .filter(|card| card.zone() == ObjectZone::Battlefield)
            else {
                return false;
            };
            if card.controller() == Some(active) {
                excluded.insert(*id);
                false
            } else {
                true
            }
        });
        let mut additional: BTreeSet<ObjectId> = evaluated.objects.values()
            .filter(|source| source.controller.is_some_and(|controller| controller != active)
                && source.has_battlefield_rule(|ability| {
                    matches!(ability, crate::characteristics::StaticContinuousAbility::UntapRule { rule: crate::characteristics::StaticUntapRule::UntapDuringOtherPlayersUntap, .. })
                }))
            .map(|source| source.object)
            .collect();
        for source in evaluated.objects.values() {
            let Some(source_controller) = source.controller else {
                continue;
            };
            if source_controller == active || source.zone != ObjectZone::Battlefield {
                continue;
            }
            for ability in source.active_battlefield_rules() {
                if let crate::characteristics::StaticContinuousAbility::UntapRule {
                    rule:
                        crate::characteristics::StaticUntapRule::UntapMatchingDuringOtherPlayersUntap(
                            selector,
                        ),
                    ..
                } = ability
                {
                    for candidate in evaluated.objects.values() {
                        if candidate.zone == ObjectZone::Battlefield
                            && crate::characteristics::selector_matches_permanent(
                                next.window.cards(),
                                &evaluated,
                                source.object,
                                source_controller,
                                selector,
                                candidate.object,
                                &next.name_rules,
                            )
                        {
                            additional.insert(candidate.object);
                        }
                    }
                }
            }
        }
        // CR 122.1d: If a permanent with a stun counter on it would become untapped,
        // instead remove a stun counter from it.
        let mut stun_occurrences = Vec::new();
        let stun_candidates: Vec<ObjectId> = next
            .window
            .cards()
            .iter()
            .filter(|object| {
                (object.controller() == Some(active) || additional.contains(&object.id()))
                    && object.tapped() == Some(true)
                    && !excluded.contains(&object.id())
                    && next
                        .counters_of_kind(object.id(), CounterKind::Stun)
                        .is_positive()
            })
            .map(GameObject::id)
            .collect();
        for id in stun_candidates {
            excluded.insert(id);
            let prev = next.counters_of_kind(id, CounterKind::Stun);
            let after = prev.minus(&1.into());
            if let Some(map) = next
                .characteristics
                .named_permanent_counters_mut(CounterKind::Stun)
            {
                if after.is_positive() {
                    map.insert(id, after.clone());
                } else {
                    map.remove(&id);
                }
            }
            stun_occurrences.push(RulesOccurrence::PermanentCounterChanged {
                object: id,
                kind: CounterKind::Stun,
                before: prev,
                after,
            });
        }
        let (window, changes) = next.window.untap_active_with_changes(
            next.window.revision(),
            &excluded,
            &additional,
        )?;
        next.window = window;
        if !changes.is_empty() || !stun_occurrences.is_empty() {
            let mut occurrences = status_occurrences(changes, StatusReason::UntapTurn);
            occurrences.extend(stun_occurrences);
            next.emit_from(
                EventCause::UntapTurn {
                    active: self.window.active_player(),
                },
                &before,
                turn,
                &mut append,
                |_, _, _| Ok(occurrences),
            )?;
        }
        Ok(RulesTransition::new(next, None, append))
    }

    /// CR 122.1d: Untap selected battlefield permanents, replacing each untap of a permanent
    /// with a stun counter by removing one stun counter instead.
    pub(super) fn untap_for_effect_with_stun(
        &mut self,
        entry: StackEntryId,
        objects: &[ObjectId],
    ) -> Result<Vec<RulesOccurrence>, EffectError> {
        let evaluated = self.evaluated_characteristics()?;
        let mut untapping = Vec::with_capacity(objects.len());
        let mut stun_occurrences = Vec::new();
        for &id in objects {
            if evaluated.get(id).is_some_and(|obj| obj.cannot_be_untapped) {
                continue;
            }
            if self.window.cards().get(id).and_then(GameObject::tapped) == Some(true)
                && self.counters_of_kind(id, CounterKind::Stun).is_positive()
            {
                let prev = self.counters_of_kind(id, CounterKind::Stun);
                let after = prev.minus(&1.into());
                if let Some(map) = self
                    .characteristics
                    .named_permanent_counters_mut(CounterKind::Stun)
                {
                    if after.is_positive() {
                        map.insert(id, after.clone());
                    } else {
                        map.remove(&id);
                    }
                }
                stun_occurrences.push(RulesOccurrence::PermanentCounterChanged {
                    object: id,
                    kind: CounterKind::Stun,
                    before: prev,
                    after,
                });
            } else {
                untapping.push(id);
            }
        }
        let mut occurrences = if untapping.is_empty() {
            Vec::new()
        } else {
            let (window, changes) = self.window.untap_for_effect(entry, &untapping)?;
            self.window = window;
            status_occurrences(changes, StatusReason::Effect)
        };
        occurrences.extend(stun_occurrences);
        Ok(occurrences)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        mana::FloatingMana,
        occurrence::{RulesRecord, TriggerError},
        turn::TurnStart,
    };

    fn completed_cleanup(positive: bool) -> (EffectWindow, TurnScheduler, TurnScheduler) {
        let a = SeatId(0);
        let b = SeatId(1);
        let c = SeatId(2);
        let old = TurnScheduler::new(
            TurnStart {
                number: 1,
                step: Step::Cleanup,
            },
            false,
        )
        .unwrap();
        let amount = if positive { 2 } else { 0 };
        let state = EffectWindow::new(GameSeed::with_uniform_life(
            vec![b, c, a],
            c,
            vec![],
            vec![],
            vec![
                FloatingMana {
                    player: b,
                    mana_type: ManaType::White,
                    amount: amount.into(),
                },
                FloatingMana {
                    player: c,
                    mana_type: ManaType::Green,
                    amount: amount.into(),
                },
                FloatingMana {
                    player: a,
                    mana_type: ManaType::Colourless,
                    amount: 0.into(),
                },
            ],
            20.into(),
            CombatRules::UnrestrictedFreeForAll,
            crate::test_support::creature_types::synthetic_vocabulary(),
            std::sync::Arc::new(spellwake_contracts::names::NameRules::literal_names()),
        ))
        .unwrap();
        // Disclosed prepared calendar resource unit, then actual cleanup/check completion.
        let state = state
            .enter_turn_step(state.revision(), c, false, false)
            .unwrap()
            .into_state();
        let (
            state,
            RuleResult::Complete {
                completion: RuleCompletion::Checks(completed),
                ..
            },
            _,
        ) = state
            .begin_cleanup(state.revision(), &old)
            .unwrap()
            .into_parts()
        else {
            panic!()
        };
        let next = old
            .complete_work(WorkCompleted::Cleanup {
                priority_required: completed.performed() || completed.waiting_triggers(),
            })
            .unwrap();
        (state, old, next)
    }

    #[test]
    fn zero_only_pool_clear_is_bookkeeping_and_allocates_only_the_real_step_event() {
        let (state, old, next) = completed_cleanup(false);
        let clocks = (
            state.revision(),
            state.window.revision(),
            state.window.cards().revision(),
        );
        let (state, (), append) = state
            .enter_calendar(state.revision(), Some(&old), &next, SeatId(0))
            .unwrap()
            .into_parts();
        assert!(state.mana.is_empty());
        assert_eq!(state.revision(), clocks.0 + 2);
        assert_eq!(state.window.revision(), clocks.1 + 2);
        assert_eq!(state.window.cards().revision(), clocks.2 + 1);
        let [RulesRecord::Event(event)] = append.records.as_slice() else {
            panic!()
        };
        assert!(
            matches!(event.cause, EventCause::StepBegan { position } if position.step == Step::Untap)
        );
        assert_eq!(event.occurrences.len(), 1);
    }

    #[test]
    fn late_step_identity_failure_discards_the_prior_tentative_whole_pool_clear() {
        let (mut state, old, next) = completed_cleanup(true);
        state.triggers.set_test_next_event(u64::MAX - 1);
        let saved = state.clone();
        assert_eq!(
            state.enter_calendar(state.revision(), Some(&old), &next, SeatId(0)),
            Err(EffectError::Trigger(TriggerError::EventIdentityExhausted))
        );
        assert_eq!(state, saved);
        assert_eq!(state.window.active_player(), SeatId(2));
        assert_eq!(state.mana(SeatId(1), ManaType::White), 2.into());
        assert_eq!(state.mana(SeatId(2), ManaType::Green), 2.into());
    }
}
