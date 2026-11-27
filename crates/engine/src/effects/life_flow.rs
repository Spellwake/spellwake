use super::*;
use crate::{
    characteristics::{ResolvedDuration, ResolvedPart, ResolvedParts},
    life::LifeAdjustment,
    occurrence::{EventCause, RulesOccurrence},
    stack_window::StackEntry,
};

impl EffectWindow {
    pub(super) fn life_gain_prohibitions(
        &self,
        current: &crate::characteristics::EvaluatedCharacteristics,
    ) -> Result<BTreeSet<SeatId>, EffectError> {
        let mut prohibited = BTreeSet::new();
        for active in &self.temporary_rules {
            if let crate::program::TemporaryRuleKind::PlayerCantGainLife { .. } = &active.rule {
                prohibited.extend(active.resolved_players.iter().copied());
            } else if let Some(ability) = self.active_temporary_static(active, current) {
                match ability {
                    crate::characteristics::StaticContinuousAbility::Restriction {
                        rule: crate::characteristics::StaticRestriction::PlayersCannotGainLife,
                        ..
                    } => {
                        prohibited.extend(self.window.participants().iter().copied());
                    }
                    crate::characteristics::StaticContinuousAbility::Restriction {
                        rule: crate::characteristics::StaticRestriction::OpponentsCannotGainLife,
                        ..
                    } => {
                        prohibited.extend(
                            self.combat_rules
                                .opponents(self.window.participants(), active.controller)?,
                        );
                    }
                    _ => {}
                }
            }
        }
        for source in current
            .objects
            .values()
            .filter(|permanent| permanent.zone == ObjectZone::Battlefield)
        {
            let forbids_all = Self::permanent_has_restriction(source, |rule| {
                matches!(
                    rule,
                    crate::characteristics::StaticRestriction::PlayersCannotGainLife
                )
            });
            if forbids_all {
                prohibited.extend(self.window.participants().iter().copied());
            }
            let forbids_gain = Self::permanent_has_restriction(source, |rule| {
                matches!(
                    rule,
                    crate::characteristics::StaticRestriction::OpponentsCannotGainLife
                )
            });
            if forbids_gain && let Some(controller) = source.controller {
                prohibited.extend(
                    self.combat_rules
                        .opponents(self.window.participants(), controller)?,
                );
            }
        }
        Ok(prohibited)
    }

    /// Players whose life total currently can't change (CR 119.7-119.8: Platinum Emperion, Teferi's Protection).
    pub(super) fn life_total_cant_change_players(
        &self,
        current: &crate::characteristics::EvaluatedCharacteristics,
    ) -> BTreeSet<SeatId> {
        let mut frozen = BTreeSet::new();
        for active in &self.temporary_rules {
            if let Some(ability) = self.active_temporary_static(active, current)
                && matches!(
                    ability,
                    crate::characteristics::StaticContinuousAbility::Restriction {
                        rule:
                            crate::characteristics::StaticRestriction::ControllerLifeTotalCantChange,
                        ..
                    }
                )
            {
                frozen.insert(active.controller);
            }
        }
        for source in current
            .objects
            .values()
            .filter(|permanent| permanent.zone == ObjectZone::Battlefield)
        {
            if let Some(controller) = source.controller
                && Self::permanent_has_restriction(source, |rule| {
                    matches!(
                        rule,
                        crate::characteristics::StaticRestriction::ControllerLifeTotalCantChange
                    )
                })
            {
                frozen.insert(controller);
            }
        }
        frozen
    }

    fn restrict_life_gains(
        &self,
        current: &crate::characteristics::EvaluatedCharacteristics,
        adjustments: &mut BTreeMap<SeatId, LifeAdjustment>,
        declined: &BTreeSet<(Option<ObjectId>, crate::characteristics::StaticAbilityId)>,
    ) -> Result<super::event_replacement::LifeReplacementOutcome, EffectError> {
        let mut outcome = super::event_replacement::LifeReplacementOutcome::default();
        let frozen = self.life_total_cant_change_players(current);
        adjustments.retain(|player, _| !frozen.contains(player));
        let prohibited = self.life_gain_prohibitions(current)?;
        adjustments.retain(|player, adjustment| {
            !prohibited.contains(player)
                || match adjustment {
                    LifeAdjustment::Gain(amount) => !amount.is_positive(),
                    LifeAdjustment::Set(total) => self
                        .life
                        .get(*player)
                        .is_none_or(|before| *total <= *before),
                    LifeAdjustment::Lose(_) => true,
                }
        });
        for (&player, adjustment) in adjustments.iter_mut() {
            let is_gain = matches!(adjustment, LifeAdjustment::Gain(_));
            let is_loss = matches!(adjustment, LifeAdjustment::Lose(_));
            if !is_gain && !is_loss {
                continue;
            }
            let original_adjustment = adjustment.clone();
            let amount = match adjustment {
                LifeAdjustment::Gain(a) | LifeAdjustment::Lose(a) => a,
                LifeAdjustment::Set(_) => continue,
            };
            if !amount.is_positive() {
                continue;
            }
            let mut total_add = 0i64;
            let mut factors: Vec<u8> = Vec::new();
            let mut lose_instead = false;
            let mut skip_event = false;
            for active in &self.temporary_rules {
                if let Some(ability) = self.active_temporary_static(active, current) {
                    match ability {
                        crate::characteristics::StaticContinuousAbility::Modifier {
                            rule:
                                crate::characteristics::StaticModifierRule::ModifyLifeGain {
                                    player: relation,
                                    add,
                                    multiply,
                                    lose_instead: invert,
                                },
                            ..
                        } if is_gain => {
                            let applies = match relation {
                                crate::program::PlayerRelation::Controller => {
                                    player == active.controller
                                }
                                crate::program::PlayerRelation::Opponent => self
                                    .combat_rules
                                    .opponents(self.window.participants(), active.controller)?
                                    .contains(&player),
                                crate::program::PlayerRelation::AnyParticipant => true,
                            };
                            if applies {
                                total_add = total_add.saturating_add(i64::from(*add));
                                if *multiply > 1 {
                                    factors.push(*multiply);
                                }
                                if *invert {
                                    lose_instead = true;
                                }
                            }
                        }
                        crate::characteristics::StaticContinuousAbility::EventReplacement {
                            id,
                            event,
                            condition,
                            action,
                        } => {
                            let rel_opt = match event {
                                crate::characteristics::ReplaceableEvent::LifeGain {
                                    player: rel,
                                } if is_gain => Some(rel),
                                crate::characteristics::ReplaceableEvent::LifeLoss {
                                    player: rel,
                                } if is_loss => Some(rel),
                                _ => None,
                            };
                            if let Some(relation) = rel_opt {
                                let applies = match relation {
                                    crate::program::AffectedPlayers::One(
                                        crate::program::ResolvingPlayer::Controller,
                                    ) => player == active.controller,
                                    crate::program::AffectedPlayers::EachOpponent
                                    | crate::program::AffectedPlayers::EachOtherPlayer => self
                                        .combat_rules
                                        .opponents(self.window.participants(), active.controller)?
                                        .contains(&player),
                                    crate::program::AffectedPlayers::EachPlayer
                                    | crate::program::AffectedPlayers::One(_) => true,
                                };
                                let cond_ok = condition.as_deref().is_none_or(|cond| {
                                    active.source_object.is_some_and(|src| {
                                        let ctx = crate::characteristics::WorldEvalCtx {
                                            active_player: self.window.active_player(),
                                            step: self.current_step,
                                            participants: self.window.participants(),
                                            life: Some(&self.life),
                                            combat: self.combat.as_ref(),
                                            cards: self.window.cards(),
                                            programs: &self.programs,
                                            state: &self.characteristics,
                                            objects: &current.objects,
                                            source: src,
                                            controller: active.controller,
                                            name_rules: &self.name_rules,
                                            trigger_subject: None,
                                            related_player: Some(player),
                                            defending_player: None,
                                        };
                                        match cond {
                                            crate::characteristics::StaticSourceCondition::Eval(
                                                rc,
                                            ) => crate::characteristics::eval_world_condition(
                                                &ctx, rc,
                                            ),
                                            _ => true,
                                        }
                                    })
                                });
                                if applies && cond_ok {
                                    match action {
                                        crate::characteristics::StaticReplacementAction::ModifyAmount {
                                            add,
                                            multiply,
                                        } => {
                                            if let crate::program::ValueExpr::Literal(v) = add.as_ref()
                                                && let Some(u) = v.to_usize()
                                            {
                                                total_add = total_add.saturating_add(
                                                    i64::try_from(u).unwrap_or(i64::MAX),
                                                );
                                            }
                                            if *multiply > 1 {
                                                factors.push(*multiply);
                                            }
                                        }
                                        crate::characteristics::StaticReplacementAction::Skip => {
                                            skip_event = true;
                                        }
                                        crate::characteristics::StaticReplacementAction::ReplaceWithBody {
                                            optional,
                                            body,
                                        } => {
                                            let key = (active.source_object, *id);
                                            if !self.active_event_replacements.contains(&key)
                                                && !declined.contains(&key)
                                            {
                                                let rep = super::event_replacement::EventBodyReplacement {
                                                    source: active.source_object,
                                                    ability: *id,
                                                    controller: active.controller,
                                                    affected_player: player,
                                                    optional: *optional,
                                                    body: (**body).clone(),
                                                };
                                                if *optional {
                                                    if outcome.optional.is_none() {
                                                        outcome.optional = Some((
                                                            player,
                                                            original_adjustment.clone(),
                                                            rep,
                                                        ));
                                                        skip_event = true;
                                                    }
                                                } else {
                                                    outcome.mandatory.push(rep);
                                                    skip_event = true;
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }
            for source in current
                .objects
                .values()
                .filter(|perm| perm.zone == ObjectZone::Battlefield)
            {
                let Some(source_ctrl) = source.controller else {
                    continue;
                };
                for ability in source
                    .printed_text
                    .continuous
                    .iter()
                    .chain(&source.printed_text.additional_battlefield_abilities)
                {
                    let active = source.abilities.continuous == Some(ability.id())
                        || source
                            .abilities
                            .additional_battlefield_abilities
                            .contains(&ability.id());
                    if !active {
                        continue;
                    }
                    match ability {
                        crate::characteristics::StaticContinuousAbility::Modifier {
                            rule:
                                crate::characteristics::StaticModifierRule::ModifyLifeGain {
                                    player: relation,
                                    add,
                                    multiply,
                                    lose_instead: invert,
                                },
                            ..
                        } if is_gain => {
                            let applies = match relation {
                                crate::program::PlayerRelation::Controller => player == source_ctrl,
                                crate::program::PlayerRelation::Opponent => self
                                    .combat_rules
                                    .opponents(self.window.participants(), source_ctrl)?
                                    .contains(&player),
                                crate::program::PlayerRelation::AnyParticipant => true,
                            };
                            if applies {
                                total_add = total_add.saturating_add(i64::from(*add));
                                if *multiply > 1 {
                                    factors.push(*multiply);
                                }
                                if *invert {
                                    lose_instead = true;
                                }
                            }
                        }
                        crate::characteristics::StaticContinuousAbility::Modifier {
                            rule:
                                crate::characteristics::StaticModifierRule::ConditionalModifyLifeGain {
                                    condition,
                                    player: relation,
                                    add,
                                    multiply,
                                },
                            ..
                        } if is_gain => {
                            let applies = match relation {
                                crate::program::PlayerRelation::Controller => player == source_ctrl,
                                crate::program::PlayerRelation::Opponent => self
                                    .combat_rules
                                    .opponents(self.window.participants(), source_ctrl)?
                                    .contains(&player),
                                crate::program::PlayerRelation::AnyParticipant => true,
                            };
                            if applies {
                                let ctx = crate::characteristics::WorldEvalCtx {
                                    active_player: self.window.active_player(),
                                    step: self.current_step,
                                    participants: self.window.participants(),
                                    life: Some(&self.life),
                                    combat: self.combat.as_ref(),
                                    cards: self.window.cards(),
                                    programs: &self.programs,
                                    state: &self.characteristics,
                                    objects: &current.objects,
                                    source: source.object,
                                    controller: source_ctrl,
                                    name_rules: &self.name_rules,
                                    trigger_subject: None,
                                    related_player: None,
                                    defending_player: None,
                                };
                                if crate::characteristics::eval_world_condition(&ctx, condition) {
                                    total_add = total_add.saturating_add(i64::from(*add));
                                    if *multiply > 1 {
                                        factors.push(*multiply);
                                    }
                                }
                            }
                        }
                        crate::characteristics::StaticContinuousAbility::EventReplacement {
                            id,
                            event,
                            condition,
                            action,
                        } => {
                            let rel_opt = match event {
                                crate::characteristics::ReplaceableEvent::LifeGain {
                                    player: rel,
                                } if is_gain => Some(rel),
                                crate::characteristics::ReplaceableEvent::LifeLoss {
                                    player: rel,
                                } if is_loss => Some(rel),
                                _ => None,
                            };
                            if let Some(relation) = rel_opt {
                                let applies = match relation {
                                    crate::program::AffectedPlayers::One(
                                        crate::program::ResolvingPlayer::Controller,
                                    ) => player == source_ctrl,
                                    crate::program::AffectedPlayers::EachOpponent
                                    | crate::program::AffectedPlayers::EachOtherPlayer => self
                                        .combat_rules
                                        .opponents(self.window.participants(), source_ctrl)?
                                        .contains(&player),
                                    crate::program::AffectedPlayers::EachPlayer
                                    | crate::program::AffectedPlayers::One(_) => true,
                                };
                                let ctx = crate::characteristics::WorldEvalCtx {
                                    active_player: self.window.active_player(),
                                    step: self.current_step,
                                    participants: self.window.participants(),
                                    life: Some(&self.life),
                                    combat: self.combat.as_ref(),
                                    cards: self.window.cards(),
                                    programs: &self.programs,
                                    state: &self.characteristics,
                                    objects: &current.objects,
                                    source: source.object,
                                    controller: source_ctrl,
                                    name_rules: &self.name_rules,
                                    trigger_subject: None,
                                    related_player: Some(player),
                                    defending_player: None,
                                };
                                let cond_ok = condition.as_deref().is_none_or(|cond| match cond {
                                    crate::characteristics::StaticSourceCondition::Eval(rc) => {
                                        crate::characteristics::eval_world_condition(&ctx, rc)
                                    }
                                    _ => true,
                                });
                                if applies && cond_ok {
                                    match action {
                                        crate::characteristics::StaticReplacementAction::ModifyAmount {
                                            add,
                                            multiply,
                                        } => {
                                            let val = crate::characteristics::eval_world_value_expr(
                                                &ctx, add,
                                            );
                                            if val.is_positive()
                                                && let Some(u) = val.to_usize()
                                            {
                                                total_add = total_add.saturating_add(
                                                    i64::try_from(u).unwrap_or(i64::MAX),
                                                );
                                            }
                                            if *multiply > 1 {
                                                factors.push(*multiply);
                                            }
                                        }
                                        crate::characteristics::StaticReplacementAction::Skip => {
                                            skip_event = true;
                                        }
                                        crate::characteristics::StaticReplacementAction::ReplaceWithBody {
                                            optional,
                                            body,
                                        } => {
                                            let key = (Some(source.object), *id);
                                            if !self.active_event_replacements.contains(&key)
                                                && !declined.contains(&key)
                                            {
                                                let rep = super::event_replacement::EventBodyReplacement {
                                                    source: Some(source.object),
                                                    ability: *id,
                                                    controller: source_ctrl,
                                                    affected_player: player,
                                                    optional: *optional,
                                                    body: (**body).clone(),
                                                };
                                                if *optional {
                                                    if outcome.optional.is_none() {
                                                        outcome.optional = Some((
                                                            player,
                                                            original_adjustment.clone(),
                                                            rep,
                                                        ));
                                                        skip_event = true;
                                                    }
                                                } else {
                                                    outcome.mandatory.push(rep);
                                                    skip_event = true;
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }
            if skip_event {
                *amount = 0.into();
                continue;
            }
            if total_add != 0 {
                *amount = amount.plus(&RulesInteger::from(total_add)).max(0.into());
            }
            for factor in factors {
                *amount = amount.times(&RulesInteger::from(i64::from(factor)));
            }
            if is_gain && lose_instead {
                *adjustment = LifeAdjustment::Lose(amount.clone());
            }
        }
        adjustments.retain(|_, adjustment| match adjustment {
            LifeAdjustment::Gain(a) | LifeAdjustment::Lose(a) => a.is_positive(),
            LifeAdjustment::Set(_) => true,
        });
        Ok(outcome)
    }

    /// Each opponent loses life simultaneously. The controller gains the total actually lost.
    pub(super) fn drain_opponents(
        &mut self,
        entry: StackEntryId,
        instruction: usize,
        controller: SeatId,
        amount: &RulesInteger,
        append: &mut RulesJournal,
    ) -> Result<(), EffectError> {
        let losses = self
            .combat_rules
            .opponents(self.window.participants(), controller)?
            .into_iter()
            .map(|player| (player, LifeAdjustment::Lose(amount.clone())))
            .collect();
        let before = self.rules_snapshot(None)?;
        let changed = self
            .life
            .apply_adjustments(&losses)
            .map_err(|_| EffectError::InconsistentProgram)?;
        let gained = changed
            .iter()
            .filter(|change| change.kind == crate::life::LifeChangeKind::Loss)
            .fold(RulesInteger::from(0), |total, change| {
                total.plus(&change.amount)
            });
        if !changed.is_empty() {
            self.emit_from(
                EventCause::Instruction { entry, instruction },
                &before,
                None,
                append,
                |_, _, _| {
                    Ok(changed
                        .into_iter()
                        .map(RulesOccurrence::LifeChanged)
                        .collect())
                },
            )?;
        }
        if gained.is_positive() {
            self.adjust_life_at_cut(
                entry,
                instruction,
                controller,
                &LifeAdjustment::Gain(gained),
                append,
            )?;
        }
        Ok(())
    }

    pub(super) fn pay_life_at_cut(
        &mut self,
        player: SeatId,
        amount: RulesInteger,
        cause: EventCause,
        turn: Option<&TurnScheduler>,
        append: &mut RulesJournal,
    ) -> Result<(), EffectError> {
        if amount == 0.into() {
            return Ok(());
        }
        if !amount.is_positive()
            || !self.window.participants().contains(&player)
            || self.life.get(player).is_none_or(|total| total < &amount)
        {
            return Err(EffectError::InvalidCastResource);
        }
        let before = self.rules_snapshot(turn)?;
        if self
            .life_total_cant_change_players(&before.view().evaluated)
            .contains(&player)
        {
            return Err(EffectError::InvalidCastResource);
        }
        if matches!(
            cause,
            EventCause::ActivatedAbilityCost { .. } | EventCause::CastLifePayment { .. }
        ) && before.view().evaluated.life_and_creature_costs_prohibited()
        {
            return Err(EffectError::InvalidCastResource);
        }
        let changes = self
            .life
            .apply_adjustments(&BTreeMap::from([(player, LifeAdjustment::Lose(amount))]))
            .map_err(|_| EffectError::InconsistentProgram)?;
        self.emit_from(cause, &before, turn, append, |_, _, _| {
            Ok(changes
                .into_iter()
                .map(RulesOccurrence::LifeChanged)
                .collect())
        })
    }

    /// Resolve life gain belonging to a mana ability, with no fabricated stack entry.
    pub(super) fn gain_life_from_mana(
        &mut self,
        source: ObjectId,
        ability: crate::mana::ManaAbilityRef,
        player: SeatId,
        amount: &RulesInteger,
        append: &mut RulesJournal,
    ) -> Result<(), EffectError> {
        let before = self.rules_snapshot(None)?;
        let mut adjustments = BTreeMap::from([(player, LifeAdjustment::Gain(amount.clone()))]);
        let outcome =
            self.restrict_life_gains(&before.view().evaluated, &mut adjustments, &BTreeSet::new())?;
        if let Some((opt_player, opt_adj, _)) = outcome.optional {
            adjustments.insert(opt_player, opt_adj);
        }
        let changes = self
            .life
            .apply_adjustments(&adjustments)
            .map_err(|_| EffectError::InconsistentProgram)?;
        if !changes.is_empty() {
            self.emit_from(
                EventCause::ManaAbilityActivated { source, ability },
                &before,
                None,
                append,
                |_, _, _| {
                    Ok(changes
                        .into_iter()
                        .map(RulesOccurrence::LifeChanged)
                        .collect())
                },
            )?;
        }
        for rep in outcome.mandatory {
            self.execute_inline_replacement_body(
                &rep,
                EventCause::ManaAbilityActivated { source, ability },
                append,
            )?;
        }
        Ok(())
    }
    pub(super) fn adjust_life_at_cut(
        &mut self,
        entry: StackEntryId,
        instruction: usize,
        player: SeatId,
        adjustment: &LifeAdjustment,
        append: &mut RulesJournal,
    ) -> Result<(), EffectError> {
        self.adjust_players_at_cut(
            entry,
            instruction,
            BTreeMap::from([(player, adjustment.clone())]),
            append,
        )
    }
    /// Apply a direct life adjustment as one event across the current opponents.
    pub(super) fn adjust_each_opponent_life_at_cut(
        &mut self,
        entry: StackEntryId,
        instruction: usize,
        controller: SeatId,
        adjustment: &LifeAdjustment,
        controller_adjustment: Option<&LifeAdjustment>,
        append: &mut RulesJournal,
    ) -> Result<(), EffectError> {
        let participants = self.window.participants();
        let opponents = if participants.contains(&controller) {
            self.combat_rules.opponents(participants, controller)?
        } else {
            participants.to_vec()
        };
        let mut adjustments: BTreeMap<_, _> = opponents
            .into_iter()
            .map(|player| (player, adjustment.clone()))
            .collect();
        if let Some(value) = controller_adjustment {
            adjustments.insert(controller, value.clone());
        }
        self.adjust_players_at_cut(entry, instruction, adjustments, append)
    }

    pub(super) fn exchange_life_with_source_toughness(
        &mut self,
        entry: &StackEntry,
        instruction: usize,
        append: &mut RulesJournal,
    ) -> Result<(), EffectError> {
        let StackKind::ActivatedAbility { source } = entry.kind() else {
            return Err(EffectError::InconsistentProgram);
        };
        let before = self.rules_snapshot(None)?;
        let current = &before.view().evaluated;
        let source = source.id();
        let Some(toughness) = current
            .get(source)
            .filter(|object| object.zone == ObjectZone::Battlefield && object.is_creature())
            .and_then(|object| object.power_toughness.as_ref())
            .map(|stats| stats.derived.toughness.clone())
        else {
            return Ok(());
        };
        let controller = entry.controller();
        let Some(life) = self.life.get(controller).cloned() else {
            return Ok(());
        };
        if toughness > life && self.life_gain_prohibitions(current)?.contains(&controller) {
            return Ok(());
        }
        let effect = self.characteristics.add_effect(
            entry.id(),
            source,
            ResolvedParts(vec![ResolvedPart::SetToughness(life)]),
            ResolvedDuration::NoStatedDuration,
        )?;
        let life_changes = self
            .life
            .apply_adjustments(&[(controller, LifeAdjustment::Set(toughness))].into())
            .map_err(|_| EffectError::InconsistentProgram)?;
        self.emit_from(
            EventCause::Instruction {
                entry: entry.id(),
                instruction,
            },
            &before,
            None,
            append,
            |old, new, _| {
                let appearances =
                    characteristic_appearance_changes(old, new, effect.affected.iter().copied())?;
                let mut occurrences = life_changes
                    .into_iter()
                    .map(RulesOccurrence::LifeChanged)
                    .collect::<Vec<_>>();
                occurrences.push(RulesOccurrence::ContinuousEffectCreated {
                    effect,
                    appearances,
                });
                Ok(occurrences)
            },
        )
    }

    pub(super) fn adjust_players_at_cut(
        &mut self,
        entry: StackEntryId,
        instruction: usize,
        adjustments: BTreeMap<SeatId, LifeAdjustment>,
        append: &mut RulesJournal,
    ) -> Result<(), EffectError> {
        self.adjust_players_at_cut_with_declined(
            entry,
            instruction,
            adjustments,
            BTreeSet::new(),
            append,
        )
    }

    pub(super) fn adjust_players_at_cut_with_declined(
        &mut self,
        entry: StackEntryId,
        instruction: usize,
        mut adjustments: BTreeMap<SeatId, LifeAdjustment>,
        declined: BTreeSet<(Option<ObjectId>, crate::characteristics::StaticAbilityId)>,
        append: &mut RulesJournal,
    ) -> Result<(), EffectError> {
        adjustments.retain(|player, _| self.window.participants().contains(player));
        if adjustments.is_empty() {
            return Ok(());
        }
        let before = self.rules_snapshot(None)?;
        let outcome =
            self.restrict_life_gains(&before.view().evaluated, &mut adjustments, &declined)?;
        let pending_opt = if let Some((opt_player, opt_adj, candidate)) = outcome.optional {
            if self.pending_work.is_some() && self.pending_life_replacement.is_none() {
                Some((opt_player, opt_adj, candidate))
            } else {
                adjustments.insert(opt_player, opt_adj);
                None
            }
        } else {
            None
        };
        let changes = self
            .life
            .apply_adjustments(&adjustments)
            .map_err(|_| EffectError::InconsistentProgram)?;
        if !changes.is_empty() {
            self.emit_from(
                EventCause::Instruction { entry, instruction },
                &before,
                None,
                append,
                |_, _, _| {
                    Ok(changes
                        .into_iter()
                        .map(RulesOccurrence::LifeChanged)
                        .collect())
                },
            )?;
        }
        for rep in outcome.mandatory {
            self.execute_inline_replacement_body(
                &rep,
                EventCause::Instruction { entry, instruction },
                append,
            )?;
        }
        if let Some((opt_player, opt_adj, candidate)) = pending_opt {
            self.pending_life_replacement =
                Some(Box::new(super::event_replacement::PendingLifeReplacement {
                    entry,
                    instruction,
                    player: opt_player,
                    adjustment: opt_adj,
                    candidate,
                    remaining: BTreeMap::new(),
                    declined,
                }));
        }
        Ok(())
    }
}
