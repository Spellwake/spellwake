//! Mana activations made while paying for an attacker declaration (CR 508.1, 734.1).
use super::*;
use crate::{
    mana::{ManaAbilityRef, ManaRestriction, ManaType},
    object_identity::{ObjectId, ObjectZone},
    occurrence::{RulesOccurrence, RulesRecord},
};
use spellwake_contracts::RulesInteger;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

/// One mana activation within the currently offered attack-cost decision.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct AttackManaId(pub u64);

/// One completed activation and the earlier mana activations it spent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttackManaChoice {
    pub id: AttackManaId,
    pub source: ObjectId,
    pub ability: ManaAbilityRef,
    pub dependencies: BTreeSet<AttackManaId>,
    pub reversible: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RecordedInput {
    actor: SeatId,
    command: GameCommand,
    issued: crate::effects::AttackAllocationCut,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Activation {
    choice: AttackManaChoice,
    commands: Vec<RecordedInput>,
    outputs: Vec<crate::mana::ManaProductionRecord>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Units {
    Ordinary,
    Restricted(ManaRestriction),
    SpendingEffect(u64),
}

type Channel = (SeatId, ManaType, Units);
type Amounts = BTreeMap<Channel, RulesInteger>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct AttackManaHistory {
    /// The staged attacker taps remain present until retained activations are reconstructed.
    start: Arc<GameState>,
    activations: Vec<Activation>,
    /// Oldest units first within each independently spendable pool.
    producers: BTreeMap<Channel, BTreeMap<Option<AttackManaId>, RulesInteger>>,
}

fn start_command(command: &GameCommand) -> Option<(ObjectId, ManaAbilityRef)> {
    match command {
        GameCommand::ActivateMana { source, ability }
        | GameCommand::ActivateHybridMana {
            source, ability, ..
        }
        | GameCommand::ActivateManaWithCostObjects {
            source, ability, ..
        }
        | GameCommand::ActivateManaWithPayment {
            source, ability, ..
        }
        | GameCommand::ActivateManaWithBundle {
            source, ability, ..
        }
        | GameCommand::ActivateManaForPlayer {
            source, ability, ..
        }
        | GameCommand::ActivateManaWithRestrictedPayment {
            source, ability, ..
        } => Some((*source, *ability)),
        _ => None,
    }
}

fn balances(window: &EffectWindow) -> Amounts {
    let mut amounts = Amounts::new();
    for &seat in window.window().participants() {
        for kind in ManaType::ALL {
            let count = window.unrestricted_mana(seat, kind);
            if count.is_positive() {
                amounts.insert((seat, kind, Units::Ordinary), count);
            }
        }
    }
    for (&(seat, kind, restriction), count) in window.restricted_mana() {
        amounts.insert((seat, kind, Units::Restricted(restriction)), count.clone());
    }
    for (id, lot) in window.floating_mana_lots() {
        let ordinary = amounts
            .entry((lot.player, lot.mana_type, Units::Ordinary))
            .or_insert_with(|| 0.into());
        *ordinary = ordinary.minus(&lot.remaining);
        amounts.insert(
            (lot.player, lot.mana_type, Units::SpendingEffect(id)),
            lot.remaining.clone(),
        );
    }
    amounts
}

fn protected(occurrence: &RulesOccurrence) -> bool {
    match occurrence {
        RulesOccurrence::LibraryCardRevealed { .. } | RulesOccurrence::Shuffled(_) => true,
        RulesOccurrence::ZoneChange(movement) => {
            matches!(movement.after.object.zone(), ObjectZone::Library(_))
                || (matches!(movement.before.object.zone(), ObjectZone::Library(_))
                    && movement.after.object.zone() != ObjectZone::Stack)
        }
        _ => false,
    }
}

impl AttackManaHistory {
    fn new(start: &GameState) -> Self {
        let producers = balances(start.execution())
            .into_iter()
            .map(|(channel, amount)| (channel, [(None, amount)].into()))
            .collect();
        Self {
            start: Arc::new(start.clone()),
            activations: Vec::new(),
            producers,
        }
    }

    fn record(
        &mut self,
        before: &GameState,
        input: GameInput,
        candidate: &NativeCandidate,
    ) -> Result<(), GameError> {
        if let Some((source, ability)) = start_command(&input.command) {
            let id = AttackManaId(
                self.activations
                    .len()
                    .try_into()
                    .map_err(|_| GameError::RevisionExhausted)?,
            );
            self.activations.push(Activation {
                choice: AttackManaChoice {
                    id,
                    source,
                    ability,
                    reversible: true,
                    dependencies: BTreeSet::new(),
                },
                commands: Vec::new(),
                outputs: Vec::new(),
            });
        }
        let activation = self
            .activations
            .last_mut()
            .ok_or(EffectError::InconsistentProgram)?;
        activation.commands.push(RecordedInput {
            actor: input.actor,
            command: input.command,
            issued: before.execution.attack_allocation_cut(),
        });
        let old = balances(before.execution());
        let new = balances(candidate.state().execution());
        let mut production = Amounts::new();
        for record in &candidate.rules_journal().records {
            let RulesRecord::Event(event) = record else {
                continue;
            };
            for row in &event.occurrences {
                if protected(&row.value) {
                    activation.choice.reversible = false;
                }
                let RulesOccurrence::ManaProduced {
                    production: output, ..
                } = &row.value
                else {
                    continue;
                };
                activation.outputs.push(output.clone());
                let unit = output
                    .restriction
                    .map_or(Units::Ordinary, Units::Restricted);
                for (&kind, amount) in &output.produced {
                    let sum = production
                        .entry((output.recipient.unwrap_or(output.controller), kind, unit))
                        .or_insert_with(|| 0.into());
                    *sum = sum.plus(amount);
                }
            }
        }
        if let Some(output) = candidate
            .mana_production()
            .filter(|p| p.produced.is_empty())
        {
            activation.outputs.push(output.clone());
        }
        // Production with a spending effect is also reported in the color total.
        // Identify its issued lot so the original explicit payment stays binding.
        for (&(seat, kind, unit), count) in &new {
            if !matches!(unit, Units::SpendingEffect(_)) || old.contains_key(&(seat, kind, unit)) {
                continue;
            }
            let ordinary = production
                .entry((seat, kind, Units::Ordinary))
                .or_insert_with(|| 0.into());
            *ordinary = ordinary.minus(count);
            production.insert((seat, kind, unit), count.clone());
        }
        let channels: BTreeSet<_> = old
            .keys()
            .chain(new.keys())
            .chain(production.keys())
            .copied()
            .collect();
        for channel in channels {
            let produced = production
                .get(&channel)
                .cloned()
                .unwrap_or_else(|| 0.into());
            let mut spent = old
                .get(&channel)
                .cloned()
                .unwrap_or_else(|| 0.into())
                .plus(&produced)
                .minus(&new.get(&channel).cloned().unwrap_or_else(|| 0.into()));
            let producers = self.producers.entry(channel).or_default();
            for (producer, remaining) in producers.iter_mut() {
                if !spent.is_positive() {
                    break;
                }
                let debit = spent.clone().min(remaining.clone());
                if debit.is_positive()
                    && let Some(id) = producer
                    && *id != activation.choice.id
                {
                    activation.choice.dependencies.insert(*id);
                }
                spent = spent.minus(&debit);
                *remaining = remaining.minus(&debit);
            }
            if spent != 0.into() {
                return Err(EffectError::InconsistentProgram.into());
            }
            if produced.is_positive() {
                let sum = producers
                    .entry(Some(activation.choice.id))
                    .or_insert_with(|| 0.into());
                *sum = sum.plus(&produced);
            }
            producers.retain(|_, count| count.is_positive());
        }
        if !candidate.shuffle_journal().is_empty() {
            activation.choice.reversible = false;
        }
        Ok(())
    }

    fn allowed(&self, requested: &[AttackManaId]) -> Result<BTreeSet<AttackManaId>, GameError> {
        let reversed: BTreeSet<_> = requested.iter().copied().collect();
        let allowed = reversed.len() == requested.len()
            && reversed.iter().all(|id| {
                self.activations
                    .iter()
                    .any(|a| a.choice.id == *id && a.choice.reversible)
            })
            && self.activations.iter().all(|a| {
                reversed.contains(&a.choice.id) || a.choice.dependencies.is_disjoint(&reversed)
            });
        if allowed {
            Ok(reversed)
        } else {
            Err(EffectError::InvalidCastResource.into())
        }
    }

    fn maximal_undo(&self) -> Vec<AttackManaId> {
        let mut keep = BTreeSet::new();
        for a in self.activations.iter().rev() {
            if !a.choice.reversible || keep.contains(&a.choice.id) {
                keep.insert(a.choice.id);
                keep.extend(&a.choice.dependencies);
            }
        }
        self.activations
            .iter()
            .filter_map(|a| (!keep.contains(&a.choice.id)).then_some(a.choice.id))
            .collect()
    }
}

impl GameState {
    /// Accepted activations during the current declaration, in original payment order.
    pub fn attack_mana_choices(&self) -> impl Iterator<Item = &AttackManaChoice> {
        self.attack_mana
            .iter()
            .flat_map(|history| history.activations.iter().map(|a| &a.choice))
    }

    pub(super) fn advance_with_attack_mana(
        &self,
        input: GameInput,
    ) -> Result<NativeCandidate, GameError> {
        if self.execution.is_paying_attack_cost()
            && let GameCommand::Reply { choice, .. } = &input.command
        {
            let selected = match choice {
                DecisionReply::RedeclareKeepingMana => Some(Vec::new()),
                DecisionReply::RedeclareReversingMana(ids) => Some(ids.clone()),
                DecisionReply::RedeclareAttackers => Some(
                    self.attack_mana
                        .as_ref()
                        .map_or_else(Vec::new, |history| history.maximal_undo()),
                ),
                _ => None,
            };
            if let Some(ids) = selected {
                return self.reverse_attack_mana(input, &ids);
            }
        }
        let recording = (self.execution.is_paying_attack_cost()
            && start_command(&input.command).is_some())
            || ((self.execution.has_attack_mana_child()
                || (self.counter_replacement.is_some() && self.attack_mana.is_some()))
                && matches!(input.command, GameCommand::Reply { .. }));
        if !recording && self.attack_mana.is_none() {
            return self.advance_with_counter_replacement(input);
        }
        let mut result = self.advance_with_counter_replacement(input.clone())?;
        if recording {
            let mut history = self
                .attack_mana
                .clone()
                .unwrap_or_else(|| Arc::new(AttackManaHistory::new(self)));
            Arc::make_mut(&mut history).record(self, input, &result)?;
            result.next.attack_mana = Some(history);
        } else if !result.next.execution.is_paying_attack_cost()
            && !result.next.execution.has_attack_mana_child()
        {
            result.next.attack_mana = None;
        }
        Ok(result)
    }

    fn reverse_attack_mana(
        &self,
        mut input: GameInput,
        requested: &[AttackManaId],
    ) -> Result<NativeCandidate, GameError> {
        let GameCommand::Reply { choice, .. } = &mut input.command else {
            return Err(GameError::WrongDecision);
        };
        // Validate the original actor, basis, and advertised decision before reconstructing anything.
        *choice = DecisionReply::RedeclareKeepingMana;
        let mut validated = self.advance_with_counter_replacement(input.clone())?;
        let Some(history) = &self.attack_mana else {
            if !requested.is_empty() {
                return Err(EffectError::InvalidCastResource.into());
            }
            for row in &mut validated.facts.rules_journal.records {
                if let RulesRecord::AttackDeclarationReversed {
                    kept_mana_abilities,
                    ..
                } = row
                {
                    *kept_mana_abilities = false;
                }
            }
            return Ok(validated);
        };
        let reversed = history.allowed(requested)?;
        let mut result = if reversed.is_empty() {
            validated
        } else if reversed.len() == history.activations.len() {
            let GameCommand::Reply { choice, .. } = &mut input.command else {
                return Err(GameError::WrongDecision);
            };
            *choice = DecisionReply::RedeclareAttackers;
            self.advance_with_counter_replacement(input)?
        } else {
            self.reconstruct_attack_mana(history, &reversed, input)?
        };
        result.next.attack_mana = None;
        for row in &mut result.facts.rules_journal.records {
            if let RulesRecord::AttackDeclarationReversed {
                kept_mana_abilities,
                reversed_mana,
                ..
            } = row
            {
                *kept_mana_abilities = history.activations.len() != reversed.len();
                *reversed_mana = reversed.clone();
            }
        }
        Ok(result)
    }

    fn reconstruct_attack_mana(
        &self,
        history: &AttackManaHistory,
        reversed: &BTreeSet<AttackManaId>,
        mut input: GameInput,
    ) -> Result<NativeCandidate, GameError> {
        let mut state = (*history.start).clone();
        for activation in &history.activations {
            if reversed.contains(&activation.choice.id) {
                continue;
            }
            state.execution.begin_retained_mana(&activation.outputs);
            for cmd in &activation.commands {
                state.execution.retain_attack_allocation_cut(&cmd.issued);
                let mut command = cmd.command.clone();
                if let GameCommand::Reply { decision, .. } = &mut command {
                    *decision = state.pending.as_ref().ok_or(GameError::NoDecision)?.id;
                }
                state = state
                    .advance_with_counter_replacement(GameInput {
                        basis: state.basis.clone(),
                        actor: cmd.actor,
                        command,
                    })?
                    .into_state();
            }
            state.execution.finish_retained_mana()?;
        }
        state
            .execution
            .retain_attack_allocation_cut(&self.execution.attack_allocation_cut());
        state
            .execution
            .restore_attack_revision(self.execution.revision())?;
        state.basis = self.basis.clone();
        state.next_decision = self.next_decision;
        let pending = state.pending.as_mut().ok_or(GameError::NoDecision)?;
        pending.frame = state
            .execution
            .pending_resolution_decision()
            .ok_or(GameError::NoDecision)?
            .into();
        pending.basis = state.basis.clone();
        input.command = GameCommand::Reply {
            decision: pending.id,
            choice: DecisionReply::RedeclareKeepingMana,
        };
        state.advance_with_counter_replacement(input)
    }
}
