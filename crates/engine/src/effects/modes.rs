//! Spell modes are chosen on announcement and performed in printed order (CR 700.2).
use super::*;
use crate::{
    mana::CastMode,
    program::SpellDefinition,
    targets::{BoundTarget, TargetLegality},
};

/// Compiled instruction stream, mode ranges, and bound target choices for a modal selection.
type ResolvedModeSelection = (Vec<Instruction>, Vec<SelectedMode>, Vec<TargetChoice>);

/// The half-open instruction range belongs to one occurrence of one printed mode.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct SelectedMode {
    pub printed: u8,
    reported: bool,
    start: usize,
    end: usize,
    pub target: Option<(TargetId, BodyTarget)>,
    extra_targets: Vec<(TargetId, BodyTarget)>,
    pub(super) clause: Option<TargetClause>,
}

impl SelectedMode {
    /// Adjust mode boundaries when an instruction at `at` is replaced by `inserted` instructions.
    pub(super) fn shift_for_splice(&mut self, at: usize, inserted: usize) {
        let adjust = |pos: usize| -> usize {
            if pos > at {
                pos.saturating_add(inserted).saturating_sub(1)
            } else {
                pos
            }
        };
        self.start = adjust(self.start);
        self.end = adjust(self.end);
    }

    /// An instruction segment introduced by splice carries targets but has no chosen mode number.
    pub(super) fn splice(
        start: usize,
        end: usize,
        mut targets: impl Iterator<Item = (TargetId, BodyTarget)>,
    ) -> Self {
        Self {
            printed: 0,
            reported: false,
            start,
            end,
            target: targets.next(),
            extra_targets: targets.collect(),
            clause: None,
        }
    }
    pub(super) fn targets(&self) -> impl Iterator<Item = (TargetId, BodyTarget)> + '_ {
        self.target.iter().chain(&self.extra_targets).copied()
    }
}

impl EffectWindow {
    /// Legal surviving objects in the original multi-target clause. CR 608.2b.
    pub(super) fn legal_permanent_targets(&self) -> Result<BTreeSet<ObjectId>, EffectError> {
        self.legal_object_targets_at(
            self.resolution_cursor()
                .map(|cursor| (cursor.entry, cursor.instruction)),
        )
    }

    pub(super) fn legal_object_targets_at(
        &self,
        position: Option<(StackEntryId, usize)>,
    ) -> Result<BTreeSet<ObjectId>, EffectError> {
        let assessment = self
            .window
            .target_assessment()
            .ok_or(EffectError::WrongResolutionFrame)?;
        let mode = position.and_then(|(entry, index)| self.instruction_mode(entry, index));
        assessment
            .results()
            .iter()
            .filter(|row| {
                row.legality == TargetLegality::Legal
                    && mode.is_none_or(|mode| mode.targets().any(|(id, _)| id == row.choice.id))
            })
            .map(|row| match row.choice.target {
                TargetRef::Object(object) => Ok(object),
                _ => Err(EffectError::InconsistentProgram),
            })
            .collect()
    }

    pub(super) fn select_modes(
        &self,
        definition: &SpellDefinition,
        choices: &[CastMode],
        entwined: bool,
        current: &EvaluatedCharacteristics,
        source: &ObjectCharacteristics,
        actor: SeatId,
    ) -> Result<(ExecutableBody, Vec<SelectedMode>, Vec<TargetChoice>), EffectError> {
        let spec = definition
            .modes
            .as_ref()
            .ok_or(EffectError::InvalidCastResource)?;
        let bodies = std::iter::once(&definition.body)
            .chain(&spec.additional)
            .collect::<Vec<_>>();
        let bounds = if entwined {
            (bodies.len(), bodies.len())
        } else {
            let mut max_modes = usize::from(spec.maximum);
            if let Some((cond, cond_max)) = &spec.conditional_maximum {
                let lhs = self.resolve_value_expr_at(actor, &cond.left, None)?;
                let rhs = self.resolve_value_expr_at(actor, &cond.right, None)?;
                if cond.comparison.evaluate(&lhs, &rhs) {
                    max_modes = max_modes.max(usize::from(*cond_max));
                }
            }
            (usize::from(spec.minimum), max_modes)
        };
        let (body, modes, targets) = self.compose_modes_with_wrap(
            &bodies,
            choices,
            bounds,
            spec.allow_repeat,
            spec.prefix.as_deref(),
            spec.suffix.as_deref(),
            |_, body, target| {
                !Self::protected_target(
                    target,
                    &super::protection::ProtectionSource::from(source).with_controller(actor),
                    current,
                ) && self.legal_body_target_in(body, target, actor, current)
            },
        )?;
        if spec.distinct_targets
            && targets
                .iter()
                .map(|choice| choice.target)
                .collect::<BTreeSet<_>>()
                .len()
                != targets.len()
        {
            return Err(EffectError::IllegalTarget);
        }
        Ok((body, modes, targets))
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn compose_modes_with_wrap(
        &self,
        bodies: &[&ExecutableBody],
        choices: &[CastMode],
        bounds: (usize, usize),
        allow_repeat: bool,
        prefix: Option<&ExecutableBody>,
        suffix: Option<&ExecutableBody>,
        legal: impl Fn(u8, &ExecutableBody, TargetRef) -> bool,
    ) -> Result<(ExecutableBody, Vec<SelectedMode>, Vec<TargetChoice>), EffectError> {
        if !(bounds.0..=bounds.1).contains(&choices.len()) {
            return Err(EffectError::InvalidCastResource);
        }
        let mut choices = choices.to_vec();
        // Preserve announcement order between repeated instances of the same mode.
        choices.sort_by_key(|choice| choice.mode);
        if !allow_repeat && choices.windows(2).any(|pair| pair[0].mode == pair[1].mode) {
            return Err(EffectError::InvalidCastResource);
        }
        let mut modes = Vec::new();
        let mut instructions = Vec::new();
        let mut targets = Vec::new();
        let append_shifted = |ops: &[Instruction], out: &mut Vec<Instruction>| {
            let start = out.len();
            for mut op in ops.iter().cloned() {
                if let Ok(shift) = isize::try_from(start) {
                    op.shift_jumps(shift, 0);
                }
                out.push(op);
            }
        };
        if let Some(pre) = prefix {
            append_shifted(&pre.instructions, &mut instructions);
        }
        for choice in choices {
            let body = *bodies
                .get(usize::from(choice.mode))
                .ok_or(EffectError::InvalidCastResource)?;
            if body
                .instructions
                .iter()
                .any(|op| matches!(op, Instruction::FixedTargetClause(_)))
            {
                if choice.target.is_some() {
                    return Err(EffectError::UnexpectedTarget);
                }
                let base_offset = instructions.len();
                let next_target_id =
                    u32::try_from(targets.len()).map_err(|_| EffectError::InconsistentProgram)?;
                let next_clause_id =
                    u32::try_from(modes.len()).map_err(|_| EffectError::InconsistentProgram)?;
                let (expanded_ops, expanded_segments, expanded_targets) = self
                    .expand_fixed_clauses_rebased(
                        body,
                        &choice.targets,
                        base_offset,
                        next_target_id,
                        next_clause_id,
                        choice.mode,
                        true,
                        |clause, target| legal(choice.mode, clause, target),
                    )?;
                instructions.extend(expanded_ops);
                modes.extend(expanded_segments);
                targets.extend(expanded_targets);
                continue;
            }
            let mut bound = Vec::new();
            let mut clause = None;
            if body.target == BodyTarget::None {
                if choice.target.is_some() || !choice.targets.is_empty() {
                    return Err(EffectError::UnexpectedTarget);
                }
            } else {
                let (minimum, maximum, selections) =
                    if let Some((minimum, maximum)) = body.permanent_target_bounds() {
                        if choice.target.is_some() {
                            return Err(EffectError::UnexpectedTarget);
                        }
                        (minimum, maximum, choice.targets)
                    } else {
                        if !choice.targets.is_empty() {
                            return Err(EffectError::UnexpectedTarget);
                        }
                        (
                            1,
                            1,
                            vec![choice.target.ok_or(EffectError::TargetRequired)?],
                        )
                    };
                if !(minimum..=maximum).contains(&selections.len())
                    || selections.iter().collect::<BTreeSet<_>>().len() != selections.len()
                {
                    return Err(EffectError::InvalidCastResource);
                }
                let clause_id = TargetClauseId(
                    u32::try_from(modes.len()).map_err(|_| EffectError::InconsistentProgram)?,
                );
                clause = Some(TargetClause {
                    id: clause_id,
                    minimum,
                    maximum,
                });
                for target in selections {
                    if !legal(choice.mode, body, target) {
                        return Err(EffectError::IllegalTarget);
                    }
                    let id = TargetId(
                        u32::try_from(targets.len())
                            .map_err(|_| EffectError::InconsistentProgram)?,
                    );
                    targets.push(TargetChoice {
                        id,
                        clause: clause_id,
                        target,
                    });
                    bound.push((id, body.target));
                }
            }
            let mut bound = bound.into_iter();
            let target = bound.next();
            let start = instructions.len();
            append_shifted(&body.instructions, &mut instructions);
            modes.push(SelectedMode {
                printed: choice.mode,
                reported: true,
                clause,
                extra_targets: bound.collect(),
                start,
                end: instructions.len(),
                target,
            });
        }
        if let Some(post) = suffix {
            append_shifted(&post.instructions, &mut instructions);
        }
        Ok((
            ExecutableBody::new(BodyTarget::None, instructions),
            modes,
            targets,
        ))
    }

    #[allow(clippy::too_many_arguments)]
    fn expand_fixed_clauses_rebased(
        &self,
        body: &ExecutableBody,
        choices: &[TargetRef],
        base_offset: usize,
        next_target_id: u32,
        next_clause_id: u32,
        printed_mode: u8,
        report_first: bool,
        legal: impl Fn(&ExecutableBody, TargetRef) -> bool,
    ) -> Result<ResolvedModeSelection, EffectError> {
        let fights = body.instructions.iter().any(|op| {
            matches!(
                op,
                Instruction::Fight { first, second }
                    if **first == crate::program::MoveSubjectRef::RememberedFirst
                        && **second == crate::program::MoveSubjectRef::RememberedSecond
            )
        });
        if fights && matches!(choices, [first, second] if first == second) {
            return Err(EffectError::IllegalTarget);
        }
        let shared_action = fights
            || body.instructions.iter().any(|op| {
                matches!(
                    op,
                    Instruction::DealDamage(spec)
                        if matches!(
                            spec.recipients,
                            crate::toolbox::DamageTargetQuery::CreaturePowerToOther { .. }
                        )
                )
            });
        let mut choices = choices.iter();
        let mut instructions = Vec::new();
        let mut segments = Vec::new();
        let mut targets = Vec::new();
        let mut old_to_new = Vec::with_capacity(body.instructions.len() + 1);
        let mut outer_indices = Vec::new();
        let mut first_target_controller: Option<SeatId> = None;
        for op in &body.instructions {
            old_to_new.push(base_offset + instructions.len());
            if let Instruction::FixedTargetClause(clause) = op {
                let target = *choices.next().ok_or(EffectError::TargetRequired)?;
                let mut bound_clause = (**clause).clone();
                if let BodyTarget::FilteredPermanent(filter) = &mut bound_clause.target
                    && filter.uses_first_target_controller()
                {
                    let seat = first_target_controller.ok_or(EffectError::IllegalTarget)?;
                    *filter = filter.bind_first_target_controller(seat);
                }
                if !legal(&bound_clause, target) {
                    return Err(EffectError::IllegalTarget);
                }
                if first_target_controller.is_none() {
                    first_target_controller = match target {
                        TargetRef::Player(seat) => Some(seat),
                        TargetRef::Object(id) => self
                            .evaluated_characteristics()
                            .ok()
                            .and_then(|ev| ev.get(id).and_then(|p| p.controller))
                            .or_else(|| {
                                self.window.cards().get(id).and_then(GameObject::controller)
                            }),
                        _ => None,
                    };
                }
                let local_ord =
                    u32::try_from(targets.len()).map_err(|_| EffectError::InvalidCastResource)?;
                let id = TargetId(
                    next_target_id
                        .checked_add(local_ord)
                        .ok_or(EffectError::InvalidCastResource)?,
                );
                let clause_id = TargetClauseId(
                    next_clause_id
                        .checked_add(local_ord)
                        .ok_or(EffectError::InvalidCastResource)?,
                );
                targets.push(TargetChoice {
                    id,
                    clause: clause_id,
                    target,
                });
                let start = base_offset + instructions.len();
                if shared_action {
                    instructions.push(Instruction::Jump {
                        destination: start + 1,
                    });
                } else {
                    let shift =
                        isize::try_from(start).map_err(|_| EffectError::InconsistentProgram)?;
                    for mut cloned in bound_clause.instructions.iter().cloned() {
                        cloned.shift_jumps(shift, 0);
                        instructions.push(cloned);
                    }
                }
                segments.push(SelectedMode {
                    printed: printed_mode,
                    reported: report_first && segments.is_empty(),
                    clause: Some(TargetClause {
                        id: clause_id,
                        minimum: 1,
                        maximum: 1,
                    }),
                    extra_targets: vec![],
                    start,
                    end: base_offset + instructions.len(),
                    target: Some((id, bound_clause.target)),
                });
            } else {
                outer_indices.push(instructions.len());
                instructions.push(op.clone());
            }
        }
        old_to_new.push(base_offset + instructions.len());
        let remap = |dest: &mut usize| {
            if let Some(&mapped) = old_to_new.get(*dest) {
                *dest = mapped;
            }
        };
        for idx in outer_indices {
            match &mut instructions[idx] {
                Instruction::FlipCoins {
                    lose_destination: Some(destination),
                    ..
                }
                | Instruction::BranchIfFalse { destination, .. }
                | Instruction::Jump { destination } => remap(destination),
                Instruction::OfferDynamicManaPayment {
                    paid_destination,
                    declined_destination,
                    ..
                } => {
                    remap(paid_destination);
                    remap(declined_destination);
                }
                Instruction::OfferSourceCounters {
                    placed_destination,
                    declined_destination,
                    ..
                } => {
                    remap(placed_destination);
                    remap(declined_destination);
                }
                _ => {}
            }
        }
        if choices.next().is_some() {
            return Err(EffectError::UnexpectedTarget);
        }
        Ok((instructions, segments, targets))
    }

    pub(super) fn expand_fixed_clauses(
        &self,
        body: &ExecutableBody,
        choices: &[TargetRef],
        actor: SeatId,
        current: &EvaluatedCharacteristics,
        source: &ObjectCharacteristics,
    ) -> Result<(ExecutableBody, Vec<SelectedMode>, Vec<TargetChoice>), EffectError> {
        let (instructions, segments, targets) = self.expand_fixed_clauses_rebased(
            body,
            choices,
            0,
            0,
            0,
            0,
            false,
            |clause, target| {
                !Self::protected_target(
                    target,
                    &super::protection::ProtectionSource::from(source).with_controller(actor),
                    current,
                ) && self.legal_body_target_in(clause, target, actor, current)
            },
        )?;
        Ok((
            ExecutableBody::new(BodyTarget::None, instructions),
            segments,
            targets,
        ))
    }

    pub(super) fn legal_linked_target_refs(
        &self,
    ) -> Result<Option<(ObjectId, TargetRef)>, EffectError> {
        let assessment = self
            .window
            .target_assessment()
            .ok_or(EffectError::WrongResolutionFrame)?;
        let (first_id, second_id) = self
            .resolution_cursor()
            .and_then(|cursor| {
                let modes = self.stack_modes(cursor.entry);
                let mut prior = modes
                    .iter()
                    .filter(|mode| mode.end <= cursor.instruction)
                    .filter_map(|mode| mode.target.map(|(tid, _)| tid))
                    .rev();
                let second = prior.next()?;
                let first = prior.next()?;
                Some((first, second))
            })
            .unwrap_or((TargetId(0), TargetId(1)));
        let first = assessment
            .get(first_id)
            .filter(|row| row.legality == TargetLegality::Legal)
            .and_then(|row| match row.choice.target {
                TargetRef::Object(object) => Some(object),
                _ => None,
            });
        let second = assessment
            .get(second_id)
            .filter(|row| row.legality == TargetLegality::Legal)
            .map(|row| row.choice.target);
        Ok(first.zip(second))
    }

    pub(super) fn legal_linked_targets(&self) -> Result<Option<(ObjectId, ObjectId)>, EffectError> {
        let Some((first, TargetRef::Object(second))) = self.legal_linked_target_refs()? else {
            return Ok(None);
        };
        Ok(Some((first, second)))
    }

    pub(super) fn stack_modes(&self, entry: StackEntryId) -> &[SelectedMode] {
        match self.payloads.get(&entry) {
            Some(StackPayload::Instructions(payload)) => &payload.modes,
            _ => &[],
        }
    }

    /// Printed indices in resolution order, including repeated chosen modes.
    pub fn chosen_spell_modes(&self, entry: StackEntryId) -> impl Iterator<Item = u8> + '_ {
        self.stack_modes(entry)
            .iter()
            .filter(|mode| mode.reported)
            .map(|mode| mode.printed)
    }

    pub(super) fn mode_target_contract(
        &self,
        entry: StackEntryId,
        target: TargetId,
    ) -> Option<BodyTarget> {
        self.stack_modes(entry)
            .iter()
            .flat_map(SelectedMode::targets)
            .find_map(|(id, kind)| (id == target).then_some(kind))
    }

    fn instruction_mode(&self, entry: StackEntryId, instruction: usize) -> Option<&SelectedMode> {
        self.stack_modes(entry)
            .iter()
            .find(|mode| (mode.start..mode.end).contains(&instruction))
    }

    pub(super) fn instruction_target<'a>(
        &self,
        entry: &'a crate::stack_window::StackEntry,
    ) -> Result<&'a BoundTarget, EffectError> {
        self.instruction_target_at(entry, self.instruction_index())
    }

    pub(super) fn instruction_target_at<'a>(
        &self,
        entry: &'a crate::stack_window::StackEntry,
        instruction: Option<usize>,
    ) -> Result<&'a BoundTarget, EffectError> {
        let targets = entry.targets().targets();
        if let Some(mode) = instruction.and_then(|index| self.instruction_mode(entry.id(), index)) {
            let (id, _) = mode.target.ok_or(EffectError::InconsistentProgram)?;
            return targets
                .iter()
                .find(|bound| bound.choice().id == id)
                .ok_or(EffectError::InconsistentProgram);
        }
        let [bound] = targets else {
            return Err(EffectError::InconsistentProgram);
        };
        Ok(bound)
    }

    pub(super) fn mode_target_illegal(&self, entry: StackEntryId, index: usize) -> bool {
        self.instruction_mode(entry, index)
            .filter(|mode| mode.target.is_some())
            .is_some_and(|mode| {
                mode.targets().all(|(id, _)| {
                    self.window
                        .target_assessment()
                        .and_then(|assessment| assessment.get(id))
                        .is_some_and(|result| result.legality != TargetLegality::Legal)
                })
            })
    }
}
