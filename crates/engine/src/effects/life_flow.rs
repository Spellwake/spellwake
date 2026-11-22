use super::*;
use crate::{
    characteristics::{ResolvedDuration, ResolvedPart, ResolvedParts},
    life::LifeAdjustment,
    occurrence::{EventCause, RulesOccurrence},
    stack_window::StackEntry,
};

impl EffectWindow {
    pub(super) fn lose_life_for_linked_damage(
        &mut self,
        entry: StackEntryId,
        instruction: usize,
        append: &mut RulesJournal,
    ) -> Result<(), EffectError> {
        let player = self.captured_related_player()?;
        if !self.window.participants().contains(&player) {
            return Ok(());
        }
        let trigger = self
            .execution_bindings(entry)
            .and_then(|bindings| bindings.trigger.as_ref())
            .ok_or(EffectError::InconsistentProgram)?;
        let amount =
            trigger
                .causes
                .members()
                .iter()
                .try_fold(RulesInteger::from(0), |total, member| {
                    let crate::occurrence::TriggerRelated::DamageReceived { amount, .. } =
                        &member.related
                    else {
                        return Err(EffectError::InconsistentProgram);
                    };
                    Ok(total.plus(amount))
                })?;
        self.adjust_life_at_cut(
            entry,
            instruction,
            player,
            &LifeAdjustment::Lose(amount),
            append,
        )
    }

    pub(super) fn life_gain_prohibitions(
        &self,
        current: &crate::characteristics::EvaluatedCharacteristics,
    ) -> Result<BTreeSet<SeatId>, EffectError> {
        let mut prohibited = BTreeSet::new();
        for source in current
            .objects
            .values()
            .filter(|permanent| permanent.zone == ObjectZone::Battlefield)
        {
            let forbids_gain = source
                .printed_text
                .continuous
                .iter()
                .chain(&source.printed_text.additional_battlefield_abilities)
                .any(|ability| {
                    matches!(
                        ability,
                        crate::characteristics::StaticContinuousAbility::Restriction {
                            rule:
                                crate::characteristics::StaticRestriction::OpponentsCannotGainLife,
                            ..
                        }
                    ) && (source.abilities.continuous == Some(ability.id())
                        || source
                            .abilities
                            .additional_battlefield_abilities
                            .contains(&ability.id()))
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

    fn restrict_life_gains(
        &self,
        current: &crate::characteristics::EvaluatedCharacteristics,
        adjustments: &mut BTreeMap<SeatId, LifeAdjustment>,
    ) -> Result<(), EffectError> {
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
        Ok(())
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
        self.restrict_life_gains(&before.view().evaluated, &mut adjustments)?;
        let changes = self
            .life
            .apply_adjustments(&adjustments)
            .map_err(|_| EffectError::InconsistentProgram)?;
        if changes.is_empty() {
            return Ok(());
        }
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
        )
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

    pub(super) fn drain_target_and_gain(
        &mut self,
        entry: &StackEntry,
        instruction: usize,
        player: SeatId,
        amount: RulesInteger,
        append: &mut RulesJournal,
    ) -> Result<(), EffectError> {
        use crate::life::LifeChangeKind;
        let before = self.rules_snapshot(None)?;
        let controller = entry.controller();
        let gain = (self.window.participants().contains(&controller)
            && !self
                .life_gain_prohibitions(&before.view().evaluated)?
                .contains(&controller))
        .then(|| (controller, LifeChangeKind::Gain, None, amount.clone()));
        let loss = self.window.participants().contains(&player).then_some((
            player,
            LifeChangeKind::Loss,
            None,
            amount,
        ));
        let changes = self
            .life
            .apply_life_components(loss.into_iter().chain(gain))
            .map_err(|_| EffectError::InconsistentProgram)?;
        self.emit_from(
            EventCause::Instruction {
                entry: entry.id(),
                instruction,
            },
            &before,
            None,
            append,
            |_, _, _| {
                Ok(changes
                    .into_iter()
                    .map(RulesOccurrence::LifeChanged)
                    .collect())
            },
        )
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
        mut adjustments: BTreeMap<SeatId, LifeAdjustment>,
        append: &mut RulesJournal,
    ) -> Result<(), EffectError> {
        adjustments.retain(|player, _| self.window.participants().contains(player));
        if adjustments.is_empty() {
            return Ok(());
        }
        let before = self.rules_snapshot(None)?;
        self.restrict_life_gains(&before.view().evaluated, &mut adjustments)?;
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
        Ok(())
    }
}
