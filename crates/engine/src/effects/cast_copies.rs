//! Copiable source and casting decisions retained by an independent cast trigger.
use super::*;
use crate::stack_window::StackEntry;
use crate::{
    occurrence::{EventCause, RulesOccurrence, TriggerRelated},
    program::CastCopyKeyword,
};

/// An ability's last known source, targets, choices and execution bindings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapturedAbilityCopy {
    pub(super) entry: StackEntry,
    pub(super) payload: Box<InstructionPayload>,
    protection_source: super::protection::ProtectionSource,
}
impl CapturedAbilityCopy {
    pub fn original(&self) -> StackEntryId {
        self.entry.id()
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CastCopySource {
    entry: StackEntry,
    mana_value: RulesInteger,
    sacrificed_creature_power: Option<RulesInteger>,
    sacrificed_creature_toughness: Option<RulesInteger>,
    exiled_cost_creature: Option<(ObjectId, RulesInteger)>,
    sacrificed_creature_subtypes: BTreeSet<CreatureTypeId>,
    sacrificed_lki: Option<Box<crate::characteristics::ObjectCharacteristics>>,
    paid_creature_taps: usize,
    target_division: BTreeMap<TargetId, u16>,
    sacrificed_permanent_mana_value: Option<RulesInteger>,
    revealed_cost_card: Option<RevealedCostCard>,
    body: Option<ExecutableBody>,
    permanent: Option<Box<Program>>,
    target_controllers: BTreeMap<TargetId, SeatId>,
    splice_target_bodies: BTreeMap<TargetId, ExecutableBody>,
    modes: Vec<modes::SelectedMode>,
    traits: CopiableSpellTraits,
    pub(crate) prior_spells: RulesInteger,
}
impl CastCopySource {
    pub(crate) fn replicated(&self, index: usize) -> RulesInteger {
        self.entry
            .replicate_choices()
            .get(&index)
            .cloned()
            .unwrap_or_else(|| 0.into())
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct CopyRun {
    source: CastCopySource,
    total: usize,
    choices: Vec<Vec<TargetChoice>>,
    creature_targets: Option<BTreeMap<ObjectId, Vec<TargetChoice>>>,
    other_players: Option<OtherCopyPlayers>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct OtherCopyPlayers {
    remaining: std::collections::VecDeque<SeatId>,
    accepted: bool,
    requests: Vec<(SeatId, Vec<TargetChoice>)>,
}
impl EffectWindow {
    pub(super) fn capture_ability(
        &self,
        id: StackEntryId,
        evaluated: &EvaluatedCharacteristics,
    ) -> Result<CapturedAbilityCopy, EffectError> {
        let entry = self
            .window
            .entries()
            .iter()
            .find(|entry| entry.id() == id)
            .ok_or(EffectError::InconsistentProgram)?;
        let Some(StackPayload::Instructions(payload)) = self.payloads.get(&id) else {
            return Err(EffectError::InconsistentProgram);
        };
        Ok(CapturedAbilityCopy {
            entry: entry.clone(),
            payload: payload.clone(),
            protection_source: self.spell_or_ability_source(entry, evaluated),
        })
    }

    pub(super) fn delayed_ability_source(
        &self,
        entry: StackEntryId,
    ) -> Result<std::sync::Arc<CapturedAbilityCopy>, EffectError> {
        let captured = if let Some(StackPayload::Instructions(p)) = self.payloads.get(&entry)
            && let Some(crate::occurrence::DelayedCopyAction::AbilityTwice(captured, _)) =
                &p.delayed_copy_action
        {
            captured
        } else {
            let binding = self
                .execution_bindings(entry)
                .and_then(|bindings| bindings.trigger.as_ref())
                .ok_or(EffectError::InconsistentProgram)?;
            let [member] = binding.causes.members() else {
                return Err(EffectError::InconsistentProgram);
            };
            let TriggerRelated::AbilityForCopy(captured) = &member.related else {
                return Err(EffectError::InconsistentProgram);
            };
            captured
        };
        // A retargeted original still on the stack supplies its current copiable choices.
        if self
            .window
            .entries()
            .iter()
            .any(|e| e.id() == captured.original())
        {
            return Ok(std::sync::Arc::new(self.capture_ability(
                captured.original(),
                &self.evaluated_characteristics()?,
            )?));
        }
        Ok(captured.clone())
    }

    pub(super) fn watch_ability_copy(
        &mut self,
        entry: &StackEntry,
        watch: crate::occurrence::AbilityCopyWatch,
    ) -> Result<(), EffectError> {
        let source = self
            .execution_bindings(entry.id())
            .and_then(|b| b.trigger.as_ref())
            .ok_or(EffectError::InconsistentProgram)?
            .source
            .clone();
        self.triggers
            .watch_ability_copy(source, entry.controller(), watch);
        Ok(())
    }

    pub(super) fn collect_delayed_target(
        &mut self,
        resolving: StackEntryId,
        source: &CapturedAbilityCopy,
        choice: CopyTargetChoice,
    ) -> Result<Option<Vec<TargetChoice>>, EffectError> {
        let original = self.next_target_occurrence(resolving, &source.entry)?;
        let replacement = match choice {
            CopyTargetChoice::Retain => None,
            CopyTargetChoice::Change(target) => {
                let selected = TargetChoice { target, ..original };
                let current = self.evaluated_characteristics()?;
                let relation = source
                    .payload
                    .body
                    .dependent_targets()
                    .filter(|_| original.clause == TargetClauseId(1));
                let legal = if let Some(relation) = relation {
                    let primary = self.retarget_primary(resolving, &source.entry)?;
                    self.legal_dependent_target(relation, primary, target, &current)
                        && !Self::protected_target(target, &source.protection_source, &current)
                } else {
                    self.ability_copy_target_legal(
                        &source.entry,
                        &source.payload,
                        selected,
                        &current,
                        &source.protection_source,
                    )
                };
                if !legal {
                    return Err(EffectError::IllegalTarget);
                }
                Some(selected)
            }
        };
        let changes = self.record_target_choice(
            resolving,
            replacement,
            source.entry.targets().targets().len(),
        )?;
        self.validate_dependent_retarget(&source.entry, Some(&source.payload.body), changes)
    }

    fn ability_copy_target_legal(
        &self,
        entry: &StackEntry,
        payload: &InstructionPayload,
        choice: TargetChoice,
        current: &EvaluatedCharacteristics,
        source: &super::protection::ProtectionSource,
    ) -> bool {
        let target = choice.target;
        if Self::protected_target(target, source, current) {
            return false;
        }
        if let Some(required) = &payload.spell_target_mana_value
            && !matches!(target,TargetRef::Stack(id) if self.entry_spell_mana_value(id).as_ref()==Some(required))
        {
            return false;
        }
        if let Some(controller) = payload.target_controllers.get(&choice.id)
            && !matches!(target,TargetRef::Object(id) if current.get(id).and_then(|o|o.controller)==Some(*controller))
        {
            return false;
        }
        if let Some((_, domain)) = payload
            .modes
            .iter()
            .flat_map(|mode| mode.targets())
            .find(|(id, _)| *id == choice.id)
        {
            return self.legal_target_kind_in(domain, target, entry.controller(), current);
        }
        if matches!(payload.body.target, BodyTarget::Object { zone: crate::program::TargetZone::Battlefield, filter } if filter.excludes_ability_source())
            && matches!((target,entry.kind()),(TargetRef::Object(id),StackKind::ActivatedAbility {source}|StackKind::TriggeredAbility {source}) if source.id()==id)
        {
            return false;
        }
        let source = payload
            .bindings
            .trigger
            .as_ref()
            .map(|trigger| &trigger.source);
        self.mentor_target(&payload.body, target, source, current)
            && self.legal_body_target_in(&payload.body, target, entry.controller(), current)
    }

    pub(super) fn entry_spell_mana_value(&self, id: StackEntryId) -> Option<RulesInteger> {
        let entry = self
            .window
            .entries()
            .iter()
            .chain(self.window.resolving())
            .find(|entry| entry.id() == id)?;
        if let Some(object) = self.window.spell_object(entry) {
            return self.stack_spell_mana_value(object).ok();
        }
        match entry.kind() {
            StackKind::CardSpell(_) => None,
            StackKind::SpellCopy { .. } => match self.payloads.get(&id)? {
                StackPayload::Instructions(payload) => payload.copied_mana_value.clone(),
                StackPayload::Permanent => None,
            },
            _ => None,
        }
    }

    pub(super) fn capture_copy_source(
        &self,
        original: StackEntryId,
        prior_spells: RulesInteger,
    ) -> Result<CastCopySource, EffectError> {
        let entry = self
            .window
            .entries()
            .iter()
            .chain(self.window.resolving())
            .find(|s| s.id() == original)
            .ok_or(EffectError::InconsistentProgram)?
            .clone();
        let body = self.stack_body(original).cloned();
        let permanent = if matches!(self.payloads.get(&original), Some(StackPayload::Permanent)) {
            let object = self
                .window
                .spell_object(&entry)
                .ok_or(EffectError::InconsistentProgram)?;
            Some(Box::new(
                self.copiable_program(object)
                    .cloned()
                    .ok_or(EffectError::InconsistentProgram)?,
            ))
        } else {
            None
        };
        if body.is_none() && permanent.is_none() {
            return Err(EffectError::InconsistentProgram);
        }
        let traits = self
            .stack_copiable_traits(original)
            .ok_or(EffectError::InconsistentProgram)?;
        let traits = CopiableSpellTraits {
            cannot_be_countered: traits.cannot_be_countered,
            colors: if let Some(program) = permanent.as_ref() {
                CopiableSpellTraits::from_program(program).colors
            } else {
                self.stack_copy_colors(original)
                    .cloned()
                    .ok_or(EffectError::InconsistentProgram)?
            },
            keywords: traits.keywords.clone(),
            names: traits.names.clone(),
            supertypes: traits.supertypes.clone(),
            card_types: traits.card_types.clone(),
            creature_subtypes: traits.creature_subtypes.clone(),
            spell_subtypes: traits.spell_subtypes.clone(),
            artifact_subtypes: traits.artifact_subtypes.clone(),
            enchantment_subtypes: traits.enchantment_subtypes.clone(),
            triggered: traits.triggered.to_vec(),
        };
        let payload = match self.payloads.get(&original) {
            Some(StackPayload::Instructions(p)) => Some(p),
            _ => None,
        };
        Ok(CastCopySource {
            mana_value: self
                .entry_spell_mana_value(original)
                .ok_or(EffectError::InconsistentProgram)?,
            sacrificed_permanent_mana_value: payload
                .and_then(|p| p.sacrificed_permanent_mana_value.clone()),
            sacrificed_creature_power: payload.and_then(|p| p.sacrificed_creature_power.clone()),
            sacrificed_creature_toughness: payload
                .and_then(|p| p.sacrificed_creature_toughness.clone()),
            exiled_cost_creature: payload.and_then(|p| p.exiled_cost_creature.clone()),
            sacrificed_creature_subtypes: payload
                .map(|p| p.sacrificed_creature_subtypes.clone())
                .unwrap_or_default(),
            sacrificed_lki: payload.and_then(|p| p.sacrificed_lki.clone()),
            paid_creature_taps: payload.map_or(0, |payload| payload.paid_creature_taps),
            target_division: payload
                .map(|payload| payload.target_division.clone())
                .unwrap_or_default(),
            revealed_cost_card: payload.and_then(|p| p.revealed_cost_card.clone()),
            modes: self.stack_modes(original).to_vec(),
            entry,
            body,
            permanent,
            splice_target_bodies: payload
                .map(|p| p.splice_target_bodies.clone())
                .unwrap_or_default(),
            target_controllers: payload
                .map(|p| p.target_controllers.clone())
                .unwrap_or_default(),
            traits,
            prior_spells,
        })
    }
    /// Collect each printed target occurrence in order. The final target plan is
    /// installed atomically, retaining the original incarnation for every Keep.
    pub(super) fn collect_copy_target(
        &mut self,
        resolving: StackEntryId,
        source: &CastCopySource,
        actor: SeatId,
        choice: CopyTargetChoice,
    ) -> Result<Option<Vec<TargetChoice>>, EffectError> {
        let targets = source.entry.targets().targets();
        let original = self.next_target_occurrence(resolving, &source.entry)?;
        let replacement = match choice {
            CopyTargetChoice::Retain => None,
            CopyTargetChoice::Change(target) => {
                let evaluated = self.evaluated_characteristics()?;
                let mode = source
                    .modes
                    .iter()
                    .flat_map(|mode| mode.targets())
                    .find(|(id, _)| *id == original.id);
                let legal = match mode {
                    _ if original.clause == TargetClauseId(1)
                        && source
                            .body
                            .as_ref()
                            .and_then(ExecutableBody::dependent_targets)
                            .is_some() =>
                    {
                        let relation = source
                            .body
                            .as_ref()
                            .and_then(ExecutableBody::dependent_targets)
                            .ok_or(EffectError::InconsistentProgram)?;
                        self.legal_dependent_target(
                            relation,
                            self.retarget_primary(resolving, &source.entry)?,
                            target,
                            &evaluated,
                        )
                    }
                    _ if source.splice_target_bodies.contains_key(&original.id) => self
                        .legal_body_target_in(
                            &source.splice_target_bodies[&original.id],
                            target,
                            actor,
                            &evaluated,
                        ),
                    Some((_, kind)) => self.legal_target_kind_in(kind, target, actor, &evaluated),
                    None => {
                        if let Some(body) = source.body.as_ref() {
                            self.legal_body_target_in(body, target, actor, &evaluated)
                        } else if let Some(crate::attachment::AttachmentKind::Aura(enchant)) =
                            source
                                .permanent
                                .as_ref()
                                .and_then(|p| p.permanent())
                                .and_then(|p| p.attachment)
                        {
                            self.aura_target_legal(enchant, target, actor, &evaluated)
                        } else {
                            false
                        }
                    }
                };
                if !legal
                    || Self::protected_target(
                        target,
                        &super::protection::ProtectionSource::from(&source.traits)
                            .with_controller(actor),
                        &evaluated,
                    )
                {
                    return Err(EffectError::IllegalTarget);
                }
                Some(TargetChoice { target, ..original })
            }
        };
        let changes = self.record_target_choice(resolving, replacement, targets.len())?;
        self.validate_dependent_retarget(&source.entry, source.body.as_ref(), changes)
    }

    fn next_target_occurrence(
        &self,
        resolving: StackEntryId,
        source: &StackEntry,
    ) -> Result<TargetChoice, EffectError> {
        let Some(StackPayload::Instructions(payload)) = self.payloads.get(&resolving) else {
            return Err(EffectError::WrongResolutionFrame);
        };
        source
            .targets()
            .targets()
            .get(payload.pending_copy_targets.len())
            .map(|bound| bound.choice())
            .ok_or(EffectError::UnexpectedCopyChoice)
    }

    pub(super) fn can_change_only_target(&self, entry: &StackEntry) -> Result<bool, EffectError> {
        let [bound] = entry.targets().targets() else {
            return Err(EffectError::InconsistentProgram);
        };
        let original = bound.choice();
        let current = self.evaluated_characteristics()?;
        let source = self.spell_or_ability_source(entry, &current);
        let domain = self
            .window
            .cards()
            .iter()
            .map(|object| TargetRef::Object(object.id()))
            .chain(
                self.window
                    .participants()
                    .iter()
                    .copied()
                    .map(TargetRef::Player),
            )
            .chain(
                self.window
                    .entries()
                    .iter()
                    .chain(self.window.resolving())
                    .map(|entry| TargetRef::Stack(entry.id())),
            );
        for target in domain {
            if target != original.target
                && self.existing_entry_target_legal(
                    entry,
                    TargetChoice { target, ..original },
                    &current,
                    &source,
                )?
            {
                return Ok(true);
            }
        }
        Ok(false)
    }

    pub(super) fn collect_existing_target(
        &mut self,
        resolving: StackEntryId,
        original: StackEntryId,
        choice: CopyTargetChoice,
        mandatory: bool,
    ) -> Result<Option<Vec<TargetChoice>>, EffectError> {
        let entry = self
            .window
            .entries()
            .iter()
            .find(|s| s.id() == original)
            .ok_or(EffectError::WrongResolutionFrame)?;
        let current = self.next_target_occurrence(resolving, entry)?;
        let replacement = match choice {
            CopyTargetChoice::Retain if mandatory => return Err(EffectError::IllegalTarget),
            CopyTargetChoice::Retain => None,
            CopyTargetChoice::Change(target) => {
                if mandatory && target == current.target {
                    return Err(EffectError::IllegalTarget);
                }
                let proposed = TargetChoice { target, ..current };
                let evaluated = self.evaluated_characteristics()?;
                let source = self.spell_or_ability_source(entry, &evaluated);
                let primary = if current.clause == TargetClauseId(1)
                    && self
                        .stack_body(entry.id())
                        .and_then(ExecutableBody::dependent_targets)
                        .is_some()
                {
                    Some(self.retarget_primary(resolving, entry)?)
                } else {
                    None
                };
                if !self.existing_entry_target_legal_with_primary(
                    entry, proposed, &evaluated, &source, primary,
                )? {
                    return Err(EffectError::IllegalTarget);
                }
                Some(proposed)
            }
        };
        let entry = entry.clone();
        let changes =
            self.record_target_choice(resolving, replacement, entry.targets().targets().len())?;
        self.validate_dependent_retarget(&entry, self.stack_body(entry.id()), changes)
    }

    fn retarget_primary(
        &self,
        resolving: StackEntryId,
        original: &StackEntry,
    ) -> Result<TargetRef, EffectError> {
        let chosen = match self.payloads.get(&resolving) {
            Some(StackPayload::Instructions(payload)) => {
                payload.pending_copy_targets.first().copied().flatten()
            }
            _ => return Err(EffectError::WrongResolutionFrame),
        };
        chosen
            .or_else(|| original.targets().targets().first().map(|row| row.choice()))
            .map(|row| row.target)
            .ok_or(EffectError::InconsistentProgram)
    }

    fn validate_dependent_retarget(
        &self,
        original: &StackEntry,
        body: Option<&ExecutableBody>,
        changes: Option<Vec<TargetChoice>>,
    ) -> Result<Option<Vec<TargetChoice>>, EffectError> {
        if let Some(relation) = body.and_then(ExecutableBody::dependent_targets)
            && let Some(changes) = &changes
            && let Some(new_primary) = changes.iter().find(|choice| choice.id == TargetId(0))
        {
            let targets = original.targets().targets();
            let old_primary = targets
                .first()
                .ok_or(EffectError::InconsistentProgram)?
                .choice()
                .target;
            let current = self.evaluated_characteristics()?;
            if targets.iter().map(|bound| bound.choice()).any(|choice| {
                choice.clause == TargetClauseId(1)
                    && !changes.iter().any(|change| change.id == choice.id)
                    && self.legal_dependent_target(relation, old_primary, choice.target, &current)
                    && !self.legal_dependent_target(
                        relation,
                        new_primary.target,
                        choice.target,
                        &current,
                    )
            }) {
                return Err(EffectError::IllegalTarget);
            }
        }
        Ok(changes)
    }

    fn record_target_choice(
        &mut self,
        resolving: StackEntryId,
        replacement: Option<TargetChoice>,
        count: usize,
    ) -> Result<Option<Vec<TargetChoice>>, EffectError> {
        let Some(StackPayload::Instructions(payload)) = self.payloads.get_mut(&resolving) else {
            return Err(EffectError::WrongResolutionFrame);
        };
        payload.pending_copy_targets.push(replacement);
        if payload.pending_copy_targets.len() < count {
            return Ok(None);
        }
        Ok(Some(
            std::mem::take(&mut payload.pending_copy_targets)
                .into_iter()
                .flatten()
                .collect(),
        ))
    }

    pub(super) fn finish_delayed_ability_copy(
        &mut self,
        entry: &StackEntry,
        source: &CapturedAbilityCopy,
        changes: Vec<TargetChoice>,
        append: &mut RulesJournal,
    ) -> Result<(), EffectError> {
        let index = self
            .instruction_index()
            .ok_or(EffectError::InconsistentProgram)?;
        let before = self.rules_snapshot(None)?;
        let occurrence = self.create_ability_copy(
            &source.entry,
            StackPayload::Instructions(source.payload.clone()),
            entry.controller(),
            changes,
        )?;
        self.emit_from(
            EventCause::Instruction {
                entry: entry.id(),
                instruction: index,
            },
            &before,
            None,
            append,
            |_, _, _| Ok(vec![occurrence]),
        )?;
        self.advance_instruction()
    }

    pub(super) fn create_ability_copy(
        &mut self,
        source: &StackEntry,
        payload: StackPayload,
        controller: SeatId,
        changes: Vec<TargetChoice>,
    ) -> Result<RulesOccurrence, EffectError> {
        self.window = self
            .window
            .copy_known_ability(source, controller, changes)?;
        let entry = self
            .window
            .entries()
            .last()
            .ok_or(EffectError::InconsistentProgram)?;
        let id = entry.id();
        let targets = entry
            .targets()
            .targets()
            .iter()
            .map(|target| target.choice())
            .collect();
        self.payloads.insert(id, payload);
        Ok(RulesOccurrence::AbilityCopied {
            entry: id,
            original: source.id(),
            controller,
            targets,
        })
    }
    pub(super) fn create_copy(
        &mut self,
        source: &CastCopySource,
        actor: SeatId,
        changes: Vec<TargetChoice>,
    ) -> Result<RulesOccurrence, EffectError> {
        self.window =
            self.window
                .copy_known_spell(self.window.revision(), &source.entry, actor, changes)?;
        let copy = self
            .window
            .entries()
            .last()
            .ok_or(EffectError::InconsistentProgram)?;
        let id = copy.id();
        let targets = copy
            .targets()
            .targets()
            .iter()
            .map(|t| t.choice())
            .collect();
        let traits = &source.traits;
        let mut payload = if let Some(definition) = source.permanent.as_ref() {
            self.window
                .give_copy_permanent_definition(id, definition.as_ref().clone())?;
            StackPayload::Permanent
        } else {
            StackPayload::from_body(
                source
                    .body
                    .clone()
                    .ok_or(EffectError::InconsistentProgram)?,
                Some(traits.clone()),
            )
        };
        if let StackPayload::Instructions(p) = &mut payload {
            p.modes.clone_from(&source.modes);
            p.splice_target_bodies
                .clone_from(&source.splice_target_bodies);
            p.target_controllers.clone_from(&source.target_controllers);
            p.copied_mana_value = Some(source.mana_value.clone());
            p.sacrificed_permanent_mana_value
                .clone_from(&source.sacrificed_permanent_mana_value);
            p.sacrificed_creature_power
                .clone_from(&source.sacrificed_creature_power);
            p.sacrificed_creature_toughness
                .clone_from(&source.sacrificed_creature_toughness);
            p.exiled_cost_creature
                .clone_from(&source.exiled_cost_creature);
            p.sacrificed_creature_subtypes
                .clone_from(&source.sacrificed_creature_subtypes);
            p.sacrificed_lki.clone_from(&source.sacrificed_lki);
            p.paid_creature_taps = source.paid_creature_taps;
            p.target_division.clone_from(&source.target_division);
            p.revealed_cost_card.clone_from(&source.revealed_cost_card);
        }
        self.payloads.insert(id, payload);
        Ok(RulesOccurrence::CopyCreated {
            copiable_card_types: traits.card_types.clone(),
            copiable_creature_subtypes: traits.creature_subtypes.clone(),
            copiable_spell_subtypes: traits.spell_subtypes.clone(),
            copiable_artifact_subtypes: traits.artifact_subtypes.clone(),
            copiable_enchantment_subtypes: traits.enchantment_subtypes.clone(),
            copiable_names: traits.names.clone(),
            copiable_supertypes: traits.supertypes.clone(),
            copiable_keywords: traits.keywords.clone(),
            copiable_triggered: traits.triggered.clone(),
            entry: id,
            original: source.entry.id(),
            controller: actor,
            targets,
        })
    }
    pub(super) fn copy_creature_order_len(&self, entry: StackEntryId) -> Option<usize> {
        let run = self.copy_run(entry)?;
        run.creature_targets.as_ref().map(|_| run.choices.len())
    }

    pub(super) fn copy_run(&self, entry: StackEntryId) -> Option<&CopyRun> {
        let StackPayload::Instructions(payload) = self.payloads.get(&entry)? else {
            return None;
        };
        payload.copy_run.as_ref()
    }
    pub(super) fn copying_original(&self, entry: StackEntryId) -> Option<StackEntryId> {
        self.copy_run(entry).map(|run| run.source.entry.id())
    }
    fn run_slot(&mut self, entry: StackEntryId) -> Result<&mut Option<CopyRun>, EffectError> {
        let Some(StackPayload::Instructions(payload)) = self.payloads.get_mut(&entry) else {
            return Err(EffectError::InconsistentProgram);
        };
        Ok(&mut payload.copy_run)
    }
    pub(super) fn copy_chooser(&self, entry: StackEntryId) -> Option<SeatId> {
        self.copy_run(entry)?
            .other_players
            .as_ref()?
            .remaining
            .front()
            .copied()
    }

    pub(super) fn accept_copy_offer(
        &mut self,
        entry: StackEntryId,
        actor: SeatId,
        take: bool,
    ) -> Result<(), EffectError> {
        let run = self
            .run_slot(entry)?
            .as_mut()
            .ok_or(EffectError::WrongResolutionFrame)?;
        let players = run
            .other_players
            .as_mut()
            .ok_or(EffectError::WrongResolutionFrame)?;
        if players.remaining.front() != Some(&actor) || players.accepted {
            return Err(EffectError::WrongChooser);
        }
        if take && !run.source.entry.targets().targets().is_empty() {
            players.accepted = true;
        } else {
            players.remaining.pop_front();
            if take {
                players.requests.push((actor, Vec::new()));
            }
        }
        Ok(())
    }

    pub(super) fn add_copy_choice(
        &mut self,
        entry: StackEntryId,
        actor: SeatId,
        choice: CopyTargetChoice,
    ) -> Result<(), EffectError> {
        let run = self
            .copy_run(entry)
            .ok_or(EffectError::WrongResolutionFrame)?;
        if let Some(players) = &run.other_players {
            if players.remaining.front() != Some(&actor) || !players.accepted {
                return Err(EffectError::WrongChooser);
            }
            let source = run.source.clone();
            if let Some(changes) = self.collect_copy_target(entry, &source, actor, choice)? {
                let players = self
                    .run_slot(entry)?
                    .as_mut()
                    .and_then(|run| run.other_players.as_mut())
                    .ok_or(EffectError::WrongResolutionFrame)?;
                players.requests.push((actor, changes));
                players.remaining.pop_front();
                players.accepted = false;
            }
            return Ok(());
        }
        if run.choices.len() >= run.total {
            return Err(EffectError::WrongResolutionFrame);
        }
        if run.creature_targets.is_some() {
            let CopyTargetChoice::Change(TargetRef::Object(object)) = choice else {
                return Err(EffectError::IllegalTarget);
            };
            let run = self
                .run_slot(entry)?
                .as_mut()
                .ok_or(EffectError::WrongResolutionFrame)?;
            let domain = run
                .creature_targets
                .as_mut()
                .ok_or(EffectError::WrongResolutionFrame)?;
            let choices = domain.remove(&object).ok_or(EffectError::IllegalTarget)?;
            run.choices.push(choices);
            if domain.len() == 1 {
                run.choices.extend(std::mem::take(domain).into_values());
            }
            return Ok(());
        }
        let source = run.source.clone();
        let Some(changes) = self.collect_copy_target(entry, &source, actor, choice)? else {
            return Ok(());
        };
        self.run_slot(entry)?
            .as_mut()
            .ok_or(EffectError::WrongResolutionFrame)?
            .choices
            .push(changes);
        Ok(())
    }
    /// Progress shared spell copying and issue the next optional or target choice.
    pub(super) fn progress_cast_copies(
        &mut self,
        entry: &StackEntry,
        kind: CastCopyKeyword,
        index: usize,
        append: &mut RulesJournal,
    ) -> Result<Option<EffectDecision>, EffectError> {
        if self.copy_run(entry.id()).is_none() {
            let (cast, twice) = if let Some(StackPayload::Instructions(p)) =
                self.payloads.get(&entry.id())
                && let Some(crate::occurrence::DelayedCopyAction::SpellTwice(cast, _)) =
                    &p.delayed_copy_action
            {
                (cast, true)
            } else {
                let binding = self
                    .execution_bindings(entry.id())
                    .and_then(|b| b.trigger.as_ref())
                    .ok_or(EffectError::InconsistentProgram)?;
                let [member] = binding.causes.members() else {
                    return Err(EffectError::InconsistentProgram);
                };
                let TriggerRelated::SpellCast(cast) = &member.related else {
                    return Err(EffectError::InconsistentProgram);
                };
                (cast, false)
            };
            let captured = cast
                .copy_source
                .as_deref()
                .ok_or(EffectError::InconsistentProgram)?;
            let count = match kind {
                CastCopyKeyword::CopyThisPermanentSpell | CastCopyKeyword::CopyObservedSpell => {
                    1.into()
                }
                CastCopyKeyword::OfferCopyToOtherPlayers => 0.into(),
                CastCopyKeyword::Storm => captured.prior_spells.clone(),
                CastCopyKeyword::Gravestorm => self
                    .cast_history
                    .as_ref()
                    .ok_or(EffectError::InconsistentProgram)?
                    .grave_permanents
                    .clone(),
                CastCopyKeyword::Replicate(index) => captured.replicated(index),
            };
            let total = if twice {
                2
            } else {
                count.to_usize().ok_or(EffectError::InconsistentProgram)?
            };
            let source = if self.window.entries().iter().any(|s| s.id() == cast.entry()) {
                self.capture_copy_source(cast.entry(), captured.prior_spells.clone())?
            } else {
                captured.clone()
            };
            let other_players = if kind == CastCopyKeyword::OfferCopyToOtherPlayers {
                Some(OtherCopyPlayers {
                    remaining: self
                        .window
                        .apnap_players()?
                        .filter(|player| *player != cast.controller())
                        .collect(),
                    accepted: false,
                    requests: Vec::new(),
                })
            } else {
                None
            };
            *self.run_slot(entry.id())? = Some(CopyRun {
                source,
                total,
                choices: Vec::new(),
                creature_targets: None,
                other_players,
            });
        }
        if let Some(players) = self
            .copy_run(entry.id())
            .and_then(|run| run.other_players.as_ref())
        {
            if let Some(&player) = players.remaining.front() {
                if players.accepted {
                    return Ok(Some(EffectDecision::CopyTargets(
                        self.copy_resolution_frame()?,
                    )));
                }
                let cursor = self
                    .resolution_cursor()
                    .ok_or(EffectError::WrongResolutionFrame)?
                    .clone();
                let work = OptionalEffectWork {
                    cursor,
                    chooser: player,
                    options: [OptionalEffectChoice::Take, OptionalEffectChoice::Decline].into(),
                    rebound_or_buyback: false,
                    cast: None,
                };
                self.pending_work = Some(PendingWork::OptionalEffect(Box::new(work)));
                return self
                    .pending_resolution_decision()
                    .map(Some)
                    .ok_or(EffectError::WrongResolutionFrame);
            }
            let run = self
                .run_slot(entry.id())?
                .take()
                .ok_or(EffectError::WrongResolutionFrame)?;
            let requests = run
                .other_players
                .ok_or(EffectError::InconsistentProgram)?
                .requests;
            self.finish_copies(entry, index, &run.source, requests, append)?;
            return Ok(None);
        }
        if self.progress_copy_run(entry, index, append)? {
            return Ok(Some(EffectDecision::CopyTargets(
                self.copy_resolution_frame()?,
            )));
        }
        Ok(None)
    }

    pub(super) fn progress_copy_run(
        &mut self,
        entry: &StackEntry,
        index: usize,
        append: &mut RulesJournal,
    ) -> Result<bool, EffectError> {
        let run = self
            .copy_run(entry.id())
            .ok_or(EffectError::InconsistentProgram)?;
        if !run.source.entry.targets().targets().is_empty() && run.choices.len() < run.total {
            return Ok(true);
        }
        let run = self
            .run_slot(entry.id())?
            .take()
            .ok_or(EffectError::InconsistentProgram)?;
        let mut decisions = run.choices.into_iter();
        let requests =
            (0..run.total).map(|_| (entry.controller(), decisions.next().unwrap_or_default()));
        self.finish_copies(entry, index, &run.source, requests, append)?;
        Ok(false)
    }

    fn finish_copies(
        &mut self,
        entry: &StackEntry,
        instruction: usize,
        source: &CastCopySource,
        requests: impl IntoIterator<Item = (SeatId, Vec<TargetChoice>)>,
        append: &mut RulesJournal,
    ) -> Result<(), EffectError> {
        let mut requests = requests.into_iter().peekable();
        if requests.peek().is_none() {
            return self.advance_instruction();
        }
        let mut events = Vec::new();
        events
            .try_reserve_exact(requests.size_hint().0)
            .map_err(|_| EffectError::InconsistentProgram)?;
        let before = self.rules_snapshot(None)?;
        for (controller, changes) in requests {
            events.push(self.create_copy(source, controller, changes)?);
        }
        self.emit_from(
            EventCause::Instruction {
                entry: entry.id(),
                instruction,
            },
            &before,
            None,
            append,
            |_, _, _| Ok(events),
        )?;
        self.advance_instruction()
    }
}
